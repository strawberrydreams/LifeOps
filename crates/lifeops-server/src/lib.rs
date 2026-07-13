pub mod app;
pub mod backup;
pub mod error;
pub mod routes;
pub mod state;
pub mod static_files;

use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::IpAddr;
    use std::path::Path;

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
}
