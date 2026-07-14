use crate::ServerPaths;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{ConnectOptions, Connection};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use zip::write::SimpleFileOptions;

static NEXT_SNAPSHOT_TEMP: AtomicU64 = AtomicU64::new(0);
static SNAPSHOT_PUBLISH_LOCK: Mutex<()> = Mutex::new(());
static SNAPSHOT_CREATE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub const LAST_SUCCESS_FILE: &str = ".last-backup-success";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreatedSnapshot {
    pub path: PathBuf,
    pub name: String,
    pub created_at: String,
    pub size: u64,
}

/// data/lifeops.db(일관 스냅샷) + schemas/ + views/ + categories.yaml을
/// backup_dir/lifeops-<타임스탬프>.zip으로 만들고 keep 초과분을 정리한다.
pub async fn create_snapshot(
    paths: &ServerPaths,
    backup_dir: &Path,
    keep: usize,
) -> io::Result<PathBuf> {
    create_snapshot_with_meta(paths, backup_dir, keep)
        .await
        .map(|created| created.path)
}

/// 생성부터 prune·응답 메타 캡처까지 직렬화해 동시 요청의 게시/삭제 경쟁을 막는다.
pub async fn create_snapshot_with_meta(
    paths: &ServerPaths,
    backup_dir: &Path,
    keep: usize,
) -> io::Result<CreatedSnapshot> {
    let _guard = SNAPSHOT_CREATE_LOCK.lock().await;
    create_snapshot_locked(paths, backup_dir, keep).await
}

async fn create_snapshot_locked(
    paths: &ServerPaths,
    backup_dir: &Path,
    keep: usize,
) -> io::Result<CreatedSnapshot> {
    validate_snapshot_inputs(paths, backup_dir, keep)?;
    if !paths.db_path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("백업할 DB 없음: {}", paths.db_path.display()),
        ));
    }

    let temp_id = NEXT_SNAPSHOT_TEMP.fetch_add(1, Ordering::Relaxed);
    let temp_db = backup_dir.join(format!(".snapshot-{}-{temp_id}.db", std::process::id()));
    let temp_zip = backup_dir.join(format!(".snapshot-{}-{temp_id}.zip", std::process::id()));
    let _ = std::fs::remove_file(&temp_db);
    let _ = std::fs::remove_file(&temp_zip);

    if let Err(error) = snapshot_db(&paths.db_path, &temp_db).await {
        let _ = std::fs::remove_file(&temp_db);
        return Err(error);
    }
    if let Err(error) = write_zip(&temp_zip, paths, &temp_db) {
        let _ = std::fs::remove_file(&temp_db);
        let _ = std::fs::remove_file(&temp_zip);
        return Err(error);
    }
    let _ = std::fs::remove_file(&temp_db);

    let destination =
        match publish_snapshot(&temp_zip, backup_dir, chrono::Local::now().naive_local()) {
            Ok(destination) => destination,
            Err(error) => {
                let _ = std::fs::remove_file(&temp_zip);
                return Err(error);
            }
        };
    if let Err(error) = std::fs::remove_file(&temp_zip) {
        if error.kind() == io::ErrorKind::NotFound {
            // hard-link 미지원 파일시스템의 rename 게시가 temp를 이동한 경우.
        } else {
            let _ = std::fs::remove_file(&temp_zip);
            return Err(error);
        }
    }

    prune_snapshots(backup_dir, keep)?;
    let metadata = std::fs::metadata(&destination)?;
    let name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "백업 파일명이 UTF-8이 아님"))?
        .to_owned();
    let created_at = metadata
        .modified()
        .map(chrono::DateTime::<chrono::Local>::from)?
        .to_rfc3339();
    let successful_at = chrono::Local::now().to_rfc3339();
    if let Err(error) = record_last_success(&paths.data_dir, &successful_at) {
        // snapshot은 이미 완전히 게시되었으므로 marker 실패가 성공을 뒤집지 않는다.
        tracing::warn!("마지막 백업 성공 시각 기록 실패: {error}");
    }
    Ok(CreatedSnapshot {
        path: destination,
        name,
        created_at,
        size: metadata.len(),
    })
}

fn validate_snapshot_inputs(paths: &ServerPaths, backup_dir: &Path, keep: usize) -> io::Result<()> {
    if keep == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "백업 보존 개수는 1 이상이어야 합니다",
        ));
    }
    match std::fs::symlink_metadata(&paths.categories_path) {
        Ok(metadata) if metadata.file_type().is_file() => {}
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "categories.yaml이 일반 파일이 아닙니다",
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "categories.yaml이 없습니다",
            ));
        }
        Err(error) => return Err(error),
    }

    for resource_dir in [&paths.schemas_dir, &paths.views_dir] {
        match std::fs::symlink_metadata(resource_dir) {
            Ok(metadata) if metadata.file_type().is_dir() => {}
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "리소스 경로가 일반 디렉터리가 아닙니다: {}",
                        resource_dir.display()
                    ),
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("리소스 디렉터리가 없습니다: {}", resource_dir.display()),
                ));
            }
            Err(error) => return Err(error),
        }
    }

    let canonical_backup = canonicalize_potential_path(backup_dir)?;
    for resource_dir in [&paths.schemas_dir, &paths.views_dir] {
        let canonical_resource = std::fs::canonicalize(resource_dir)?;
        if canonical_backup.starts_with(&canonical_resource) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "백업 폴더는 리소스 폴더와 같거나 그 하위일 수 없습니다: {}",
                    resource_dir.display()
                ),
            ));
        }
    }
    match std::fs::metadata(backup_dir) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "백업 경로가 디렉터리가 아닙니다",
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            std::fs::create_dir_all(backup_dir)?;
        }
        Err(error) => return Err(error),
    }
    Ok(())
}

/// 아직 없는 목적 경로는 가장 가까운 기존 조상만 canonicalize한 뒤 나머지를 결합한다.
fn canonicalize_potential_path(path: &Path) -> io::Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut cursor = absolute.as_path();
    let mut missing = Vec::new();
    loop {
        match std::fs::symlink_metadata(cursor) {
            Ok(_) => {
                let mut canonical = std::fs::canonicalize(cursor)?;
                for component in missing.iter().rev() {
                    canonical.push(component);
                }
                return Ok(canonical);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let name = cursor.file_name().ok_or(error)?;
                missing.push(name.to_os_string());
                cursor = cursor.parent().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::NotFound, "백업 경로의 기존 조상을 찾지 못함")
                })?;
            }
            Err(error) => return Err(error),
        }
    }
}

pub fn load_last_success(data_dir: &Path) -> Option<String> {
    let value = std::fs::read_to_string(data_dir.join(LAST_SUCCESS_FILE)).ok()?;
    let value = value.trim();
    chrono::DateTime::parse_from_rfc3339(value).ok()?;
    Some(value.to_owned())
}

/// 목록 조회 시 대상 폴더를 읽고 새 파일을 만들 수 있는지 확인한다.
pub fn backup_dir_accessible(backup_dir: &Path) -> bool {
    if !std::fs::metadata(backup_dir).is_ok_and(|metadata| metadata.is_dir())
        || std::fs::read_dir(backup_dir).is_err()
    {
        return false;
    }
    let id = NEXT_SNAPSHOT_TEMP.fetch_add(1, Ordering::Relaxed);
    let probe = backup_dir.join(format!(".lifeops-write-probe-{}-{id}", std::process::id()));
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
    {
        Ok(file) => {
            drop(file);
            std::fs::remove_file(probe).is_ok()
        }
        Err(_) => false,
    }
}

fn record_last_success(data_dir: &Path, value: &str) -> io::Result<()> {
    std::fs::create_dir_all(data_dir)?;
    let (temp, mut file) = loop {
        let id = NEXT_SNAPSHOT_TEMP.fetch_add(1, Ordering::Relaxed);
        let temp = data_dir.join(format!(
            "{LAST_SUCCESS_FILE}.tmp-{}-{id}",
            std::process::id()
        ));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
        {
            Ok(file) => break (temp, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    let result = (|| {
        file.write_all(value.as_bytes())?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp, data_dir.join(LAST_SUCCESS_FILE))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

/// 기존 파일을 덮어쓰지 않고 원자적으로 게시한다. 동일 초 이름이 있으면 다음 빈 초를 쓴다.
fn publish_snapshot(
    temp_zip: &Path,
    backup_dir: &Path,
    initial_time: chrono::NaiveDateTime,
) -> io::Result<PathBuf> {
    for offset in 0..=24 * 60 * 60 {
        let timestamp = initial_time + chrono::Duration::seconds(offset);
        let destination =
            backup_dir.join(format!("lifeops-{}.zip", timestamp.format("%Y%m%d-%H%M%S")));
        match std::fs::hard_link(temp_zip, &destination) {
            Ok(()) => return Ok(destination),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(link_error) => {
                let _guard = SNAPSHOT_PUBLISH_LOCK
                    .lock()
                    .map_err(|_| io::Error::other("백업 게시 잠금 손상"))?;
                if destination.try_exists()? {
                    continue;
                }
                return std::fs::rename(temp_zip, &destination)
                    .map(|()| destination)
                    .map_err(|rename_error| {
                        io::Error::new(
                            rename_error.kind(),
                            format!(
                                "백업 원자 게시 실패(hard-link: {link_error}, rename: {rename_error})"
                            ),
                        )
                    });
            }
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "24시간 범위에 사용 가능한 백업 파일명이 없음",
    ))
}

fn write_zip(destination: &Path, paths: &ServerPaths, db_snapshot: &Path) -> io::Result<()> {
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    let mut archive = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    archive.add_directory("data/", options).map_err(zip_error)?;
    archive
        .start_file("data/lifeops.db", options)
        .map_err(zip_error)?;
    copy_file_to_zip(&mut archive, db_snapshot)?;
    archive
        .add_directory("schemas/", options)
        .map_err(zip_error)?;
    add_dir(&mut archive, &paths.schemas_dir, "schemas", options)?;
    archive
        .add_directory("views/", options)
        .map_err(zip_error)?;
    add_dir(&mut archive, &paths.views_dir, "views", options)?;
    archive
        .start_file("categories.yaml", options)
        .map_err(zip_error)?;
    copy_file_to_zip(&mut archive, &paths.categories_path)?;

    let output = archive.finish().map_err(zip_error)?;
    output.sync_all()
}

fn copy_file_to_zip(archive: &mut zip::ZipWriter<std::fs::File>, source: &Path) -> io::Result<()> {
    let mut file = std::fs::File::open(source)?;
    io::copy(&mut file, archive)?;
    Ok(())
}

/// 디렉터리 아래 일반 파일을 재귀적으로, 심볼릭 링크를 따라가지 않고 추가한다.
fn add_dir(
    archive: &mut zip::ZipWriter<std::fs::File>,
    directory: &Path,
    prefix: &str,
    options: SimpleFileOptions,
) -> io::Result<()> {
    match std::fs::symlink_metadata(directory) {
        Ok(metadata) if metadata.file_type().is_dir() => {}
        Ok(_) => return Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    }
    let mut pending = vec![(directory.to_path_buf(), prefix.to_owned())];
    while let Some((current, archive_prefix)) = pending.pop() {
        let mut entries: Vec<_> = std::fs::read_dir(&current)?.collect::<Result<_, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries.into_iter().rev() {
            let file_type = entry.file_type()?;
            if file_type.is_symlink() {
                continue;
            }
            let name = entry.file_name().into_string().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "파일명을 UTF-8로 읽지 못함")
            })?;
            let archive_name = format!("{archive_prefix}/{name}");
            if file_type.is_dir() {
                pending.push((entry.path(), archive_name));
            } else if file_type.is_file() {
                archive
                    .start_file(archive_name, options)
                    .map_err(zip_error)?;
                copy_file_to_zip(archive, &entry.path())?;
            }
        }
    }
    Ok(())
}

async fn snapshot_db(source: &Path, destination: &Path) -> io::Result<()> {
    let options = SqliteConnectOptions::new()
        .filename(source)
        .read_only(true)
        .disable_statement_logging();
    let mut connection = sqlx::SqliteConnection::connect_with(&options)
        .await
        .map_err(sqlx_error)?;
    let result = sqlx::query("VACUUM INTO ?")
        .bind(destination.to_string_lossy().as_ref())
        .execute(&mut connection)
        .await
        .map_err(sqlx_error);
    let close_result = connection.close().await.map_err(sqlx_error);
    result?;
    close_result
}

fn zip_error(error: zip::result::ZipError) -> io::Error {
    io::Error::other(error)
}

fn sqlx_error(error: sqlx::Error) -> io::Error {
    io::Error::other(error)
}

/// 이름순(=시간순) 정렬 후 최신 keep개 zip만 남긴다.
pub fn prune_snapshots(backup_dir: &Path, keep: usize) -> io::Result<()> {
    let mut snapshots = Vec::new();
    for entry in std::fs::read_dir(backup_dir)? {
        let entry = entry?;
        if entry.file_type()?.is_file()
            && entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("lifeops-") && name.ends_with(".zip"))
        {
            snapshots.push(entry.path());
        }
    }
    snapshots.sort();
    if snapshots.len() > keep {
        for old in &snapshots[..snapshots.len() - keep] {
            std::fs::remove_file(old)?;
        }
    }
    Ok(())
}

/// 현재 config.json을 다시 읽어 해당 시점의 폴더·보존 개수로 백업한다.
pub async fn create_configured_snapshot(data_dir: &Path) -> io::Result<CreatedSnapshot> {
    let config = crate::config::load_config(data_dir);
    let backup_dir = config.resolved_backup_dir(data_dir);
    let paths = crate::resolve_paths(data_dir);
    create_snapshot_with_meta(&paths, &backup_dir, config.backup_keep).await
}

/// 24시간 간격으로 zip 스냅샷을 만드는 백그라운드 태스크. 매 tick 최신 config를 읽는다.
pub fn spawn_daily_backup(data_dir: PathBuf) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(24 * 60 * 60));
        loop {
            ticker.tick().await;
            match create_configured_snapshot(&data_dir).await {
                Ok(created) => tracing::info!("백업 생성: {}", created.path.display()),
                Err(error) => tracing::error!("백업 실패: {error}"),
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn 스냅샷_zip에_db와_schemas_views_categories_포함() {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::resolve_paths(dir.path());
        crate::install_seed_if_empty(&paths).unwrap();
        lifeops_core::entity::EntityStore::open(&paths.db_path)
            .await
            .unwrap();

        let backup_dir = dir.path().join("backups");
        let zip_path = create_snapshot(&paths, &backup_dir, 7).await.unwrap();
        assert!(zip_path.exists());
        assert!(zip_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("lifeops-"));
        assert_eq!(zip_path.extension().unwrap(), "zip");

        let file = std::fs::File::open(&zip_path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let names: Vec<String> = (0..archive.len())
            .map(|index| archive.by_index(index).unwrap().name().to_owned())
            .collect();
        assert!(
            names.iter().any(|name| name == "data/lifeops.db"),
            "{names:?}"
        );
        assert!(names.iter().any(|name| name == "data/"), "{names:?}");
        assert!(
            names.iter().any(|name| name.starts_with("schemas/")),
            "{names:?}"
        );
        assert!(
            names.iter().any(|name| name == "categories.yaml"),
            "{names:?}"
        );
        assert!(
            names.iter().any(|name| name.starts_with("views/")),
            "{names:?}"
        );
    }

    #[tokio::test]
    async fn 스냅샷은_db_없으면_에러() {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::resolve_paths(dir.path());
        std::fs::create_dir_all(&paths.schemas_dir).unwrap();
        let result = create_snapshot(&paths, &dir.path().join("backups"), 7).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn keep_0은_기존_zip을_지우지_않고_거부() {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::resolve_paths(dir.path());
        crate::install_seed_if_empty(&paths).unwrap();
        lifeops_core::entity::EntityStore::open(&paths.db_path)
            .await
            .unwrap();
        let existing = paths.backups_dir.join("lifeops-20260101-000000.zip");
        std::fs::write(&existing, b"existing").unwrap();

        let error = create_snapshot(&paths, &paths.backups_dir, 0)
            .await
            .unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(std::fs::read(existing).unwrap(), b"existing");
    }

    #[tokio::test]
    async fn backup_dir가_schemas와_같거나_하위면_거부() {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::resolve_paths(dir.path());
        crate::install_seed_if_empty(&paths).unwrap();
        lifeops_core::entity::EntityStore::open(&paths.db_path)
            .await
            .unwrap();

        let same_error = create_snapshot(&paths, &paths.schemas_dir, 7)
            .await
            .unwrap_err();
        assert_eq!(same_error.kind(), io::ErrorKind::InvalidInput);
        let nested = paths.schemas_dir.join("backups");
        let nested_error = create_snapshot(&paths, &nested, 7).await.unwrap_err();
        assert_eq!(nested_error.kind(), io::ErrorKind::InvalidInput);
        assert!(!nested.exists(), "거부한 중첩 폴더를 만들면 안 됨");
        assert!(!std::fs::read_dir(&paths.schemas_dir)
            .unwrap()
            .filter_map(Result::ok)
            .any(|entry| entry.file_name().to_string_lossy().contains("snapshot")));
    }

    #[tokio::test]
    async fn 빈_schemas_views도_directory_entry로_기록() {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::resolve_paths(dir.path());
        std::fs::create_dir_all(paths.db_path.parent().unwrap()).unwrap();
        std::fs::create_dir_all(&paths.schemas_dir).unwrap();
        std::fs::create_dir_all(&paths.views_dir).unwrap();
        std::fs::write(&paths.categories_path, b"[]\n").unwrap();
        lifeops_core::entity::EntityStore::open(&paths.db_path)
            .await
            .unwrap();

        let snapshot = create_snapshot(&paths, &paths.backups_dir, 7)
            .await
            .unwrap();
        let mut archive = zip::ZipArchive::new(std::fs::File::open(snapshot).unwrap()).unwrap();
        assert!(archive.by_name("schemas/").unwrap().is_dir());
        assert!(archive.by_name("views/").unwrap().is_dir());
    }

    #[tokio::test]
    async fn schemas나_views가_누락되면_snapshot을_거부() {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::resolve_paths(dir.path());
        std::fs::create_dir_all(paths.db_path.parent().unwrap()).unwrap();
        std::fs::create_dir_all(&paths.views_dir).unwrap();
        std::fs::write(&paths.categories_path, b"[]\n").unwrap();
        lifeops_core::entity::EntityStore::open(&paths.db_path)
            .await
            .unwrap();

        let error = create_snapshot(&paths, &paths.backups_dir, 7)
            .await
            .unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(!paths.backups_dir.exists());
    }

    #[tokio::test]
    async fn categories가_없으면_snapshot을_생성하지_않는다() {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::resolve_paths(dir.path());
        std::fs::create_dir_all(paths.db_path.parent().unwrap()).unwrap();
        lifeops_core::entity::EntityStore::open(&paths.db_path)
            .await
            .unwrap();

        let error = create_snapshot(&paths, &paths.backups_dir, 7)
            .await
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(!paths.backups_dir.exists());
    }

    #[test]
    fn prune_snapshots는_zip만_keep개_남긴다() {
        let dir = tempfile::tempdir().unwrap();
        let backups = dir.path().join("backups");
        std::fs::create_dir_all(&backups).unwrap();
        for index in 0..5 {
            std::fs::write(
                backups.join(format!("lifeops-2026010{index}-000000.zip")),
                b"z",
            )
            .unwrap();
        }
        std::fs::write(backups.join("lifeops-old.db"), b"d").unwrap();
        prune_snapshots(&backups, 2).unwrap();
        let mut zips: Vec<_> = std::fs::read_dir(&backups)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
            .filter(|name| name.ends_with(".zip"))
            .collect();
        zips.sort();
        assert_eq!(
            zips,
            vec!["lifeops-20260103-000000.zip", "lifeops-20260104-000000.zip"]
        );
        assert!(backups.join("lifeops-old.db").exists(), "db 파일은 보존");
    }

    #[test]
    fn 같은_초_게시도_기존_zip을_덮어쓰지_않는다() {
        let dir = tempfile::tempdir().unwrap();
        let first_temp = dir.path().join("first.tmp");
        let second_temp = dir.path().join("second.tmp");
        std::fs::write(&first_temp, b"first").unwrap();
        std::fs::write(&second_temp, b"second").unwrap();
        let initial =
            chrono::NaiveDateTime::parse_from_str("2026-01-02 03:04:05", "%Y-%m-%d %H:%M:%S")
                .unwrap();

        let first = publish_snapshot(&first_temp, dir.path(), initial).unwrap();
        let second = publish_snapshot(&second_temp, dir.path(), initial).unwrap();

        assert_eq!(first.file_name().unwrap(), "lifeops-20260102-030405.zip");
        assert_eq!(second.file_name().unwrap(), "lifeops-20260102-030406.zip");
        assert_eq!(std::fs::read(first).unwrap(), b"first");
        assert_eq!(std::fs::read(second).unwrap(), b"second");
    }

    #[cfg(unix)]
    #[test]
    fn prune_snapshots는_zip_이름의_symlink를_건드리지_않는다() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let backups = dir.path().join("backups");
        std::fs::create_dir_all(&backups).unwrap();
        let target = dir.path().join("outside.zip");
        std::fs::write(&target, b"outside").unwrap();
        let link = backups.join("lifeops-20260102-030405.zip");
        symlink(&target, &link).unwrap();

        prune_snapshots(&backups, 0).unwrap();

        assert!(std::fs::symlink_metadata(link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(std::fs::read(target).unwrap(), b"outside");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn categories_symlink는_snapshot을_거부한다() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let paths = crate::resolve_paths(dir.path());
        crate::install_seed_if_empty(&paths).unwrap();
        lifeops_core::entity::EntityStore::open(&paths.db_path)
            .await
            .unwrap();
        std::fs::remove_file(&paths.categories_path).unwrap();
        let outside = dir.path().join("outside-categories.yaml");
        std::fs::write(&outside, b"secret").unwrap();
        symlink(&outside, &paths.categories_path).unwrap();

        let error = create_snapshot(&paths, &dir.path().join("backups"), 7)
            .await
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

    #[tokio::test]
    async fn 성공_snapshot은_data_dir에_last_success를_기록() {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::resolve_paths(dir.path());
        crate::install_seed_if_empty(&paths).unwrap();
        lifeops_core::entity::EntityStore::open(&paths.db_path)
            .await
            .unwrap();

        create_snapshot(&paths, &paths.backups_dir, 7)
            .await
            .unwrap();

        let value = load_last_success(dir.path()).unwrap();
        assert!(chrono::DateTime::parse_from_rfc3339(&value).is_ok());
    }

    #[tokio::test]
    async fn configured_snapshot은_실행_직전의_경로와_keep을_사용() {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::resolve_paths(dir.path());
        crate::install_seed_if_empty(&paths).unwrap();
        lifeops_core::entity::EntityStore::open(&paths.db_path)
            .await
            .unwrap();
        crate::config::save_config(
            dir.path(),
            &crate::config::AppConfig {
                backup_dir: Some(PathBuf::from("old-backups")),
                backup_keep: 7,
                ..Default::default()
            },
        )
        .unwrap();

        let new_backups = dir.path().join("new-backups");
        std::fs::create_dir_all(&new_backups).unwrap();
        std::fs::write(new_backups.join("lifeops-20260101-000000.zip"), b"old-1").unwrap();
        std::fs::write(new_backups.join("lifeops-20260102-000000.zip"), b"old-2").unwrap();
        crate::config::save_config(
            dir.path(),
            &crate::config::AppConfig {
                backup_dir: Some(PathBuf::from("new-backups")),
                backup_keep: 1,
                ..Default::default()
            },
        )
        .unwrap();

        let created = create_configured_snapshot(dir.path()).await.unwrap();

        assert_eq!(created.path.parent().unwrap(), new_backups);
        assert!(!dir.path().join("old-backups").exists());
        let snapshots: Vec<_> = std::fs::read_dir(created.path.parent().unwrap())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry.file_name().to_string_lossy().starts_with("lifeops-")
                    && entry.file_name().to_string_lossy().ends_with(".zip")
            })
            .collect();
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].path(), created.path);
    }
}
