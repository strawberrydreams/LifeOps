pub mod app;
pub mod backup;
pub mod config;
pub mod error;
pub mod routes;
pub mod state;
pub mod static_files;

use lifeops_core::entity::EntityStore;
use lifeops_core::schema::SchemaSet;
use lifeops_core::view::PageSet;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::{fs, io};

pub type BoxErr = Box<dyn std::error::Error + Send + Sync>;

#[derive(Clone, Debug)]
pub struct RunConfig {
    pub data_dir: PathBuf,
    pub bind_addr: IpAddr,
    pub port: u16,
}

impl RunConfig {
    /// 개발용: 현재 작업 디렉터리 기준(기존 상대경로 동작 유지).
    pub fn dev() -> Self {
        RunConfig {
            data_dir: PathBuf::from("."),
            bind_addr: "0.0.0.0".parse().unwrap(),
            port: 3000,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ServerPaths {
    pub data_dir: PathBuf,
    pub schemas_dir: PathBuf,
    pub views_dir: PathBuf,
    pub categories_path: PathBuf,
    pub db_path: PathBuf,
    pub backups_dir: PathBuf,
    pub logs_dir: PathBuf,
}

#[derive(rust_embed::RustEmbed)]
#[folder = "../../seed"]
struct SeedAssets;

const SEED_INSTALL_MARKER: &str = ".seed-installing";
static NEXT_SEED_TEMP: AtomicU64 = AtomicU64::new(0);

pub fn resolve_paths(data_dir: &Path) -> ServerPaths {
    ServerPaths {
        data_dir: data_dir.to_path_buf(),
        schemas_dir: data_dir.join("schemas"),
        views_dir: data_dir.join("views"),
        categories_path: data_dir.join("categories.yaml"),
        db_path: data_dir.join("data").join("lifeops.db"),
        backups_dir: data_dir.join("backups"),
        logs_dir: data_dir.join("logs"),
    }
}

/// schemas 디렉터리가 없거나 비어있으면 번들 시드를 data_dir에 복사한다.
/// 설치했으면 true, 이미 데이터가 있으면 false.
pub fn install_seed_if_empty(paths: &ServerPaths) -> std::io::Result<bool> {
    let marker = paths.data_dir.join(SEED_INSTALL_MARKER);
    let empty = !paths.schemas_dir.exists()
        || std::fs::read_dir(&paths.schemas_dir)
            .map(|mut d| d.next().is_none())
            .unwrap_or(true);
    if !empty && !marker.exists() {
        return Ok(false);
    }

    fs::create_dir_all(&paths.data_dir)?;
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&marker)?;

    for rel in SeedAssets::iter() {
        let dest = paths.data_dir.join(rel.as_ref());
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = SeedAssets::get(rel.as_ref()).expect("임베드 시드 존재");
        write_new_seed_file(&dest, file.data.as_ref())?;
    }
    // 런타임 디렉터리 보장
    if let Some(parent) = paths.db_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::create_dir_all(&paths.backups_dir)?;
    fs::create_dir_all(&paths.logs_dir)?;
    match fs::remove_file(marker) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    Ok(true)
}

fn write_new_seed_file(path: &Path, contents: &[u8]) -> io::Result<()> {
    let temp = write_seed_temp(path, contents)?;
    publish_seed_temp(&temp, path)
}

fn write_seed_temp(path: &Path, contents: &[u8]) -> io::Result<PathBuf> {
    use std::io::Write;

    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "시드 대상의 부모 경로가 없음")
    })?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "시드 대상의 파일명이 없음"))?;
    let (temp, mut file) = loop {
        let id = NEXT_SEED_TEMP.fetch_add(1, Ordering::Relaxed);
        let temp = parent.join(format!(
            ".{}.seed-tmp-{}-{id}",
            name.to_string_lossy(),
            std::process::id()
        ));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
        {
            Ok(file) => break (temp, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    if let Err(error) = file.write_all(contents).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    Ok(temp)
}

fn publish_seed_temp(temp: &Path, path: &Path) -> io::Result<()> {
    match fs::hard_link(temp, path) {
        Ok(()) => fs::remove_file(temp),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => fs::remove_file(temp),
        Err(error) => {
            let _ = fs::remove_file(temp);
            Err(error)
        }
    }
}

/// OS 표준 앱 데이터 디렉터리(macOS ~/Library/Application Support/LifeOps).
pub fn default_data_dir() -> PathBuf {
    directories::ProjectDirs::from("", "", "LifeOps")
        .map(|d| d.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// start..=start+99 에서 최초로 바인드되는 리스너를 반환.
pub async fn bind_with_fallback(
    ip: IpAddr,
    start: u16,
) -> std::io::Result<tokio::net::TcpListener> {
    let mut last_err = None;
    for port in start..=start.saturating_add(99) {
        match tokio::net::TcpListener::bind(SocketAddr::new(ip, port)).await {
            Ok(l) => return Ok(l),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err
        .unwrap_or_else(|| std::io::Error::new(std::io::ErrorKind::AddrInUse, "가용 포트 없음")))
}

pub async fn build_state(
    config: &RunConfig,
    bound_addr: SocketAddr,
) -> Result<state::AppState, BoxErr> {
    let paths = resolve_paths(&config.data_dir);
    if let Some(parent) = paths.db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let schemas = SchemaSet::load_dir(&paths.schemas_dir)?;
    let pages = PageSet::load_dir(&paths.views_dir)?;
    let categories = lifeops_core::schema::load_categories(&paths.categories_path)?;
    let store = EntityStore::open(&paths.db_path).await?;
    Ok(state::AppState::new(
        schemas,
        pages,
        categories,
        store,
        paths.schemas_dir,
        paths.views_dir,
        paths.categories_path,
        config.data_dir.clone(),
        bound_addr,
    ))
}

/// 시드 설치 → config 로드 → config 바인드로 포트 폴백 → 상태 로드 → 백업 태스크 → (확정주소, 실행 future).
pub async fn serve(
    config: RunConfig,
) -> Result<(SocketAddr, impl std::future::Future<Output = ()>), BoxErr> {
    let paths = resolve_paths(&config.data_dir);
    install_seed_if_empty(&paths)?;
    let app_config = config::load_config(&config.data_dir);
    let listener = bind_with_fallback(app_config.bind_ip(), config.port).await?;
    let addr = listener.local_addr()?;
    let state = build_state(&config, addr).await?;
    backup::spawn_daily_backup(config.data_dir.clone());
    let app = app::build_app(state);
    let fut = async move {
        if let Err(error) = axum::serve(listener, app).await {
            tracing::error!("서버 실행 실패: {error}");
        }
    };
    Ok((addr, fut))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::build_app;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use std::net::IpAddr;
    use std::path::Path;
    use tower::ServiceExt;

    #[test]
    fn 경로_해석() {
        let p = resolve_paths(Path::new("/tmp/lo"));
        assert_eq!(p.schemas_dir, Path::new("/tmp/lo/schemas"));
        assert_eq!(p.views_dir, Path::new("/tmp/lo/views"));
        assert_eq!(p.categories_path, Path::new("/tmp/lo/categories.yaml"));
        assert_eq!(p.db_path, Path::new("/tmp/lo/data/lifeops.db"));
        assert_eq!(p.backups_dir, Path::new("/tmp/lo/backups"));
        assert_eq!(p.logs_dir, Path::new("/tmp/lo/logs"));
    }

    #[tokio::test]
    async fn 포트_점유되면_다음_포트() {
        let ip: IpAddr = "127.0.0.1".parse().unwrap();
        let first = bind_with_fallback(ip, 3000).await.unwrap();
        let p1 = first.local_addr().unwrap().port();
        let second = bind_with_fallback(ip, p1).await.unwrap();
        let p2 = second.local_addr().unwrap().port();
        assert_ne!(p1, p2);
        assert!(p2 > p1 && p2 <= p1.saturating_add(99));
    }

    #[tokio::test]
    async fn build_state_시드후_health() {
        let dir = tempfile::tempdir().unwrap();
        let config = RunConfig {
            data_dir: dir.path().to_path_buf(),
            bind_addr: "127.0.0.1".parse().unwrap(),
            port: 0,
        };
        install_seed_if_empty(&resolve_paths(&config.data_dir)).unwrap();
        let addr = "127.0.0.1:0".parse().unwrap();
        let state = build_state(&config, addr).await.unwrap();
        let app = build_app(state);
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn serve_바인드하고_접속됨() {
        let dir = tempfile::tempdir().unwrap();
        let config = RunConfig {
            data_dir: dir.path().to_path_buf(),
            bind_addr: "127.0.0.1".parse().unwrap(),
            port: 0,
        };
        let (addr, fut) = serve(config).await.unwrap();
        let handle = tokio::spawn(fut);
        let conn = tokio::net::TcpStream::connect(addr).await;
        assert!(conn.is_ok());
        handle.abort();
    }

    #[tokio::test]
    async fn serve는_config의_lan을_읽어_unspecified로_바인드() {
        let dir = tempfile::tempdir().unwrap();
        crate::config::save_config(
            dir.path(),
            &crate::config::AppConfig {
                bind_scope: crate::config::BindScope::Lan,
                backup_dir: None,
                backup_keep: 7,
            },
        )
        .unwrap();
        let config = RunConfig {
            data_dir: dir.path().to_path_buf(),
            bind_addr: "127.0.0.1".parse().unwrap(),
            port: 0,
        };
        let (addr, fut) = serve(config).await.unwrap();
        let handle = tokio::spawn(fut);
        assert!(addr.ip().is_unspecified(), "config=lan이면 0.0.0.0 바인드");
        handle.abort();
    }

    #[tokio::test]
    async fn serve는_config_없으면_localhost_기본() {
        let dir = tempfile::tempdir().unwrap();
        let config = RunConfig {
            data_dir: dir.path().to_path_buf(),
            bind_addr: "0.0.0.0".parse().unwrap(),
            port: 0,
        };
        let (addr, fut) = serve(config).await.unwrap();
        let handle = tokio::spawn(fut);
        assert!(addr.ip().is_loopback(), "config 없으면 localhost");
        handle.abort();
    }

    #[test]
    fn 빈_디렉터리에_시드_설치_후_재설치_안함() {
        let dir = tempfile::tempdir().unwrap();
        let paths = resolve_paths(dir.path());
        assert!(install_seed_if_empty(&paths).unwrap());
        assert!(paths.categories_path.exists());
        assert!(paths.schemas_dir.join("할일.yaml").exists());
        assert!(paths.schemas_dir.join("물건.yaml").exists());
        assert!(paths.schemas_dir.join("프로필.yaml").exists());
        assert!(paths.views_dir.join("홈.yaml").exists());
        assert!(paths.backups_dir.exists());
        assert!(paths.logs_dir.exists());
        // 이미 채워졌으면 재설치 안 함
        assert!(!install_seed_if_empty(&paths).unwrap());
    }

    #[test]
    fn 첫_설치는_기존_사용자_파일을_덮어쓰지_않는다() {
        let dir = tempfile::tempdir().unwrap();
        let paths = resolve_paths(dir.path());
        std::fs::create_dir_all(&paths.schemas_dir).unwrap();
        std::fs::create_dir_all(&paths.views_dir).unwrap();
        std::fs::write(&paths.categories_path, "user categories").unwrap();
        let home = paths.views_dir.join("홈.yaml");
        std::fs::write(&home, "user home").unwrap();

        assert!(install_seed_if_empty(&paths).unwrap());
        assert_eq!(
            std::fs::read_to_string(&paths.categories_path).unwrap(),
            "user categories"
        );
        assert_eq!(std::fs::read_to_string(home).unwrap(), "user home");
        assert!(paths.schemas_dir.join("할일.yaml").exists());
    }

    #[test]
    fn 진행_마커가_있으면_부분_설치를_재개한다() {
        let dir = tempfile::tempdir().unwrap();
        let paths = resolve_paths(dir.path());
        std::fs::create_dir_all(&paths.schemas_dir).unwrap();
        let existing = paths.schemas_dir.join("할일.yaml");
        std::fs::write(&existing, "user schema").unwrap();
        let marker = paths.data_dir.join(".seed-installing");
        std::fs::write(&marker, "").unwrap();

        assert!(install_seed_if_empty(&paths).unwrap());
        assert_eq!(std::fs::read_to_string(existing).unwrap(), "user schema");
        assert!(paths.schemas_dir.join("물건.yaml").exists());
        assert!(paths.schemas_dir.join("프로필.yaml").exists());
        assert!(paths.categories_path.exists());
        assert!(paths.views_dir.join("홈.yaml").exists());
        assert!(!marker.exists());
    }

    #[test]
    fn 중간_오류_후_다음_호출이_설치를_복구한다() {
        let dir = tempfile::tempdir().unwrap();
        let paths = resolve_paths(dir.path());
        std::fs::write(&paths.views_dir, "blocks view directory creation").unwrap();

        assert!(install_seed_if_empty(&paths).is_err());
        let marker = paths.data_dir.join(SEED_INSTALL_MARKER);
        assert!(marker.exists());

        std::fs::remove_file(&paths.views_dir).unwrap();
        assert!(install_seed_if_empty(&paths).unwrap());
        assert!(paths.schemas_dir.join("할일.yaml").exists());
        assert!(paths.views_dir.join("홈.yaml").exists());
        assert!(!marker.exists());
    }

    #[test]
    fn marker가_있어도_시드는_temp_완료_후에만_최종_경로로_게시된다() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("seed.yaml");
        let contents = b"complete seed";
        let marker = dir.path().join(SEED_INSTALL_MARKER);
        std::fs::write(&marker, b"").unwrap();

        let temp = write_seed_temp(&dest, contents).unwrap();
        assert!(marker.exists());
        assert!(temp.exists());
        assert!(!dest.exists(), "temp 기록 중 최종 경로가 노출되면 안 된다");

        publish_seed_temp(&temp, &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), contents);
        assert!(!temp.exists());
    }

    #[test]
    fn 최종_시드가_이미_있으면_보존하고_새_temp를_정리한다() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("seed.yaml");
        std::fs::write(&dest, b"user seed").unwrap();
        let temp = write_seed_temp(&dest, b"bundled seed").unwrap();

        publish_seed_temp(&temp, &dest).unwrap();

        assert_eq!(std::fs::read(dest).unwrap(), b"user seed");
        assert!(!temp.exists());
    }

    #[test]
    fn crash로_stale_temp가_남아도_marker_재실행은_정상_시드를_게시한다() {
        let dir = tempfile::tempdir().unwrap();
        let paths = resolve_paths(dir.path());
        std::fs::create_dir_all(&paths.schemas_dir).unwrap();
        let stale = paths.schemas_dir.join(".할일.yaml.seed-tmp-stale");
        std::fs::write(&stale, b"truncated").unwrap();
        std::fs::write(paths.data_dir.join(SEED_INSTALL_MARKER), b"").unwrap();

        assert!(install_seed_if_empty(&paths).unwrap());
        let expected = SeedAssets::get("schemas/할일.yaml").unwrap();
        assert_eq!(
            std::fs::read(paths.schemas_dir.join("할일.yaml")).unwrap(),
            expected.data.as_ref()
        );
        assert!(!paths.data_dir.join(SEED_INSTALL_MARKER).exists());
    }

    #[test]
    fn 설치된_시드는_코어_로더로_파싱된다() {
        let dir = tempfile::tempdir().unwrap();
        let paths = resolve_paths(dir.path());
        assert!(install_seed_if_empty(&paths).unwrap());

        let schemas = lifeops_core::schema::SchemaSet::load_dir(&paths.schemas_dir).unwrap();
        assert!(schemas.get("할일").is_some());
        assert!(schemas.get("물건").is_some());
        assert!(schemas.get("프로필").is_some());
        let pages = lifeops_core::view::PageSet::load_dir(&paths.views_dir).unwrap();
        assert!(pages.get("홈").is_some());
        let categories = lifeops_core::schema::load_categories(&paths.categories_path).unwrap();
        assert_eq!(categories.len(), 3);
    }
}
