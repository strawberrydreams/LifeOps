use lifeops_server::{default_data_dir, resolve_paths, serve, RunConfig};
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{ConnectOptions, Connection};
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::async_runtime::spawn;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, Url, WebviewWindow, WindowEvent, Wry};
use tauri_plugin_autostart::ManagerExt;

const AUTOSTART_INIT_MARKER: &str = ".autostart-initialized";
const PENDING_IMPORT_DIR: &str = ".pending-import";
const IMPORT_READY_MARKER: &str = ".ready";
const IMPORT_MANIFEST: &str = ".apply-manifest.json";
const IMPORT_BACKUPS_DIR: &str = ".import-backups";
const IMPORT_STAGING_PREFIX: &str = ".import-staging-";
const IMPORT_NAMES: [&str; 4] = ["schemas", "views", "data", "categories.yaml"];
const MAX_SNAPSHOT_ENTRIES: usize = 4_096;
const MAX_SNAPSHOT_UNCOMPRESSED_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_SNAPSHOT_ENTRY_BYTES: u64 = 2 * 1024 * 1024 * 1024;
static NEXT_IMPORT_ID: AtomicU64 = AtomicU64::new(0);
static IMPORT_IN_PROGRESS: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
struct ImportManifest {
    version: u8,
    backup_id: String,
    items: Vec<ImportManifestItem>,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
struct ImportManifestItem {
    name: String,
    had_target: bool,
    phase: ImportPhase,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum ImportPhase {
    Staged,
    BackedUp,
    Promoted,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = tracing_subscriber::fmt().try_init();
    let data_dir = default_data_dir();

    let mut builder = tauri::Builder::default();

    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            show_main_window(app);
        }));
        builder = builder.plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ));
    }

    builder
        .invoke_handler(tauri::generate_handler![
            get_autostart,
            set_autostart,
            open_data_dir,
            import_from_dir,
            restore_snapshot,
            relaunch_app
        ])
        .setup(move |app| {
            let open_i = MenuItem::with_id(app, "open", "열기", true, None::<&str>)?;
            let addr_i = MenuItem::with_id(app, "addr", "LAN 주소: 시작 중…", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "종료", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open_i, &addr_i, &quit_i])?;
            let icon = app.default_window_icon().cloned().ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::NotFound, "기본 트레이 아이콘 없음")
            })?;
            let _tray = TrayIconBuilder::new()
                .icon(icon)
                .menu(&menu)
                .show_menu_on_left_click(true)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "open" => show_main_window(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            // 이 호출은 위의 모든 fallible 동기 셸 구성이 성공한 뒤에 둔다. setup이
            // 실패하는 실행에서 로그인 항목만 먼저 활성화되는 부분 초기화를 막는다.
            initialize_default_autostart(app.handle(), &data_dir);

            let handle = app.handle().clone();
            let data_dir = data_dir.clone();
            let addr_i = addr_i.clone();
            spawn(async move {
                if let Err(error) = apply_pending_import(&data_dir).await {
                    let logs_dir = resolve_paths(&data_dir).logs_dir;
                    if let Some(target) = parse_target_url(
                        &startup_failure_url(
                            &format!("대기 중인 데이터 가져오기 적용 실패: {error}"),
                            &logs_dir,
                        ),
                        "가져오기 실패 URL",
                    ) {
                        if let Some(window) = handle.get_webview_window("main") {
                            navigate_when_ready(&window, target);
                        }
                    }
                    return;
                }
                let config = RunConfig {
                    data_dir: data_dir.clone(),
                    bind_addr: "0.0.0.0".parse().expect("유효한 기본 바인드 주소"),
                    port: 3000,
                };
                let result = serve(config).await;
                let target = match &result {
                    Ok((addr, _)) => {
                        parse_target_url(&format!("http://127.0.0.1:{}", addr.port()), "서버 URL")
                    }
                    Err(error) => {
                        let logs_dir = resolve_paths(&data_dir).logs_dir;
                        parse_target_url(
                            &startup_failure_url(&error.to_string(), &logs_dir),
                            "기동 실패 URL",
                        )
                    }
                };
                if let Some(target) = target {
                    if let Some(window) = handle.get_webview_window("main") {
                        navigate_when_ready(&window, target);
                    } else {
                        tracing::error!("main webview를 찾지 못해 URL로 이동할 수 없습니다");
                    }
                }
                if let Ok((addr, future)) = result {
                    if let Err(error) = addr_i.set_text(tray_address_label(addr)) {
                        tracing::error!(%error, "트레이 접속 범위 갱신 실패");
                    }
                    future.await;
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if let Err(error) = window.hide() {
                    tracing::error!(%error, "창 숨김 실패");
                }
                api.prevent_close();
            }
        })
        .run(tauri::generate_context!())
        .expect("Tauri 실행 실패");
}

#[tauri::command]
fn get_autostart(app: AppHandle) -> Result<bool, String> {
    app.autolaunch()
        .is_enabled()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn set_autostart(app: AppHandle, enabled: bool) -> Result<(), String> {
    let manager = app.autolaunch();
    if enabled {
        manager.enable().map_err(|error| error.to_string())
    } else {
        manager.disable().map_err(|error| error.to_string())
    }
}

#[tauri::command]
fn open_data_dir() -> Result<(), String> {
    open_data_dir_at(&default_data_dir())
}

fn open_data_dir_at(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    let status = std::process::Command::new("open")
        .arg(dir)
        .status()
        .map_err(|error| format!("Finder 실행 실패: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("Finder가 비정상 종료했습니다: {status}"))
    }
}

/// 실행 중인 destination SQLite는 건드리지 않고, 완전한 snapshot을 다음 시작용으로 게시한다.
#[tauri::command]
async fn import_from_dir(dir: String) -> Result<(), String> {
    let _guard = ImportGuard::acquire()?;
    stage_import(Path::new(&dir), &default_data_dir()).await
}

/// 백업 목록의 파일명으로 snapshot을 검증·추출한 뒤 기존 import 경로에 staging한다.
#[tauri::command]
async fn restore_snapshot(name: String) -> Result<(), String> {
    let name_path = Path::new(&name);
    if name.is_empty()
        || name.contains("..")
        || name.contains('/')
        || name.contains('\\')
        || name_path.components().count() != 1
        || name_path
            .file_name()
            .is_none_or(|file_name| file_name != name.as_str())
        || !name.starts_with("lifeops-")
        || !name.ends_with(".zip")
    {
        return Err("잘못된 백업 이름입니다".into());
    }

    let _guard = ImportGuard::acquire()?;
    let data_dir = default_data_dir();
    ensure_no_pending_import(&data_dir)?;
    let config = lifeops_server::config::load_config(&data_dir);
    let backup_dir = config.resolved_backup_dir(&data_dir);
    let zip_path = backup_dir.join(&name);
    let metadata = std::fs::symlink_metadata(&zip_path)
        .map_err(|error| format!("백업 파일 확인 실패: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("백업은 symlink가 아닌 일반 zip 파일이어야 합니다".into());
    }

    let staging_root = std::env::temp_dir().join(format!("lifeops-restore-{}", import_id()));
    std::fs::create_dir(&staging_root)
        .map_err(|error| format!("복원 임시 폴더 생성 실패: {error}"))?;
    let result = async {
        unpack_snapshot(&zip_path, &staging_root)?;
        stage_import(&staging_root, &data_dir).await
    }
    .await;
    if let Err(error) = std::fs::remove_dir_all(&staging_root) {
        tracing::warn!(path = %staging_root.display(), %error, "복원 임시 폴더 정리 실패");
    }
    result
}

fn ensure_no_pending_import(data_dir: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(data_dir.join(PENDING_IMPORT_DIR)) {
        Ok(_) => Err("이미 적용 대기 중인 가져오기가 있습니다".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("적용 대기 중인 가져오기 확인 실패: {error}")),
    }
}

/// zip을 `dest` 하위로 푼다. 쓰기 전에 전체 엔트리의 경로·타입·필수 layout을 검증한다.
fn unpack_snapshot(zip_path: &Path, dest: &Path) -> Result<(), String> {
    let mut archive = open_snapshot_archive(zip_path)?;
    let mut entries = HashMap::<PathBuf, bool>::new();
    let mut declared_total = 0_u64;
    let entry_count = archive.len();

    validate_snapshot_budget(entry_count, declared_total)?;

    for index in 0..entry_count {
        let entry = archive
            .by_index(index)
            .map_err(|error| format!("zip 엔트리 읽기 실패: {error}"))?;
        let relative = entry
            .enclosed_name()
            .ok_or_else(|| "안전하지 않은 zip 경로".to_string())?
            .to_path_buf();
        validate_snapshot_entry(&entry, &relative)?;
        validate_snapshot_entry_budget(entry.size())?;
        let is_dir = entry.is_dir();
        if entries.insert(relative.clone(), is_dir).is_some() {
            return Err(format!("중복 zip 엔트리: {}", relative.display()));
        }
        declared_total = declared_total
            .checked_add(entry.size())
            .ok_or_else(|| "snapshot 비압축 크기가 표현 범위를 넘습니다".to_string())?;
        validate_snapshot_budget(entry_count, declared_total)?;
    }

    validate_required_snapshot_layout(&entries)?;
    validate_snapshot_entry_conflicts(&entries)?;

    let destination_metadata = std::fs::symlink_metadata(dest)
        .map_err(|error| format!("복원 대상 폴더 확인 실패: {error}"))?;
    if destination_metadata.file_type().is_symlink() || !destination_metadata.is_dir() {
        return Err("복원 대상은 symlink가 아닌 디렉터리여야 합니다".into());
    }

    let mut extracted_total = 0_u64;
    for index in 0..entry_count {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("zip 엔트리 읽기 실패: {error}"))?;
        let relative = entry
            .enclosed_name()
            .ok_or_else(|| "안전하지 않은 zip 경로".to_string())?;
        let output = dest.join(&relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&output)
                .map_err(|error| format!("복원 디렉터리 생성 실패: {error}"))?;
            continue;
        }
        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("복원 디렉터리 생성 실패: {error}"))?;
        }
        let mut writer = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&output)
            .map_err(|error| format!("복원 파일 생성 실패: {error}"))?;
        let declared_size = entry.size();
        let remaining = MAX_SNAPSHOT_UNCOMPRESSED_BYTES
            .checked_sub(extracted_total)
            .ok_or_else(|| "snapshot 비압축 크기 제한을 초과했습니다".to_string())?;
        let entry_remaining = remaining.min(MAX_SNAPSHOT_ENTRY_BYTES);
        let mut limited = std::io::Read::take(&mut entry, entry_remaining.saturating_add(1));
        let copied = std::io::copy(&mut limited, &mut writer)
            .map_err(|error| format!("복원 파일 기록 실패: {error}"))?;
        if copied > MAX_SNAPSHOT_ENTRY_BYTES {
            return Err(format!(
                "snapshot 개별 엔트리 크기 제한({MAX_SNAPSHOT_ENTRY_BYTES} bytes)을 초과했습니다"
            ));
        }
        if copied > remaining {
            return Err(format!(
                "snapshot 비압축 크기 제한({MAX_SNAPSHOT_UNCOMPRESSED_BYTES} bytes)을 초과했습니다"
            ));
        }
        if copied != declared_size {
            return Err(format!(
                "zip 엔트리 선언 크기 불일치({}): declared={declared_size}, actual={copied}",
                relative.display()
            ));
        }
        extracted_total = extracted_total
            .checked_add(copied)
            .ok_or_else(|| "snapshot 비압축 크기가 표현 범위를 넘습니다".to_string())?;
        writer
            .sync_all()
            .map_err(|error| format!("복원 파일 동기화 실패: {error}"))?;
    }
    sync_directory(dest)?;
    Ok(())
}

fn validate_snapshot_budget(entry_count: usize, uncompressed_bytes: u64) -> Result<(), String> {
    if entry_count > MAX_SNAPSHOT_ENTRIES {
        return Err(format!(
            "snapshot 엔트리 수 제한({MAX_SNAPSHOT_ENTRIES})을 초과했습니다"
        ));
    }
    if uncompressed_bytes > MAX_SNAPSHOT_UNCOMPRESSED_BYTES {
        return Err(format!(
            "snapshot 비압축 크기 제한({MAX_SNAPSHOT_UNCOMPRESSED_BYTES} bytes)을 초과했습니다"
        ));
    }
    Ok(())
}

fn validate_snapshot_entry_budget(uncompressed: u64) -> Result<(), String> {
    if uncompressed > MAX_SNAPSHOT_ENTRY_BYTES {
        return Err(format!(
            "snapshot 개별 엔트리 크기 제한({MAX_SNAPSHOT_ENTRY_BYTES} bytes)을 초과했습니다"
        ));
    }
    Ok(())
}

fn validate_required_snapshot_layout(entries: &HashMap<PathBuf, bool>) -> Result<(), String> {
    for (path, expected_dir) in [
        ("data", true),
        ("data/lifeops.db", false),
        ("schemas", true),
        ("views", true),
        ("categories.yaml", false),
    ] {
        if entries.get(Path::new(path)) != Some(&expected_dir) {
            let expected = if expected_dir {
                "디렉터리"
            } else {
                "파일"
            };
            return Err(format!(
                "백업 zip 필수 항목 누락/타입 오류: {path} ({expected})"
            ));
        }
    }
    Ok(())
}

fn open_snapshot_archive(zip_path: &Path) -> Result<zip::ZipArchive<std::fs::File>, String> {
    #[cfg(unix)]
    let file = {
        let fd = rustix::fs::open(
            zip_path,
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC
                | rustix::fs::OFlags::NONBLOCK,
            rustix::fs::Mode::empty(),
        )
        .map_err(|error| format!("백업 zip no-follow 열기 실패: {error}"))?;
        let stat =
            rustix::fs::fstat(&fd).map_err(|error| format!("백업 zip 정보 확인 실패: {error}"))?;
        if !rustix::fs::FileType::from_raw_mode(stat.st_mode).is_file() {
            return Err("백업 zip이 일반 파일이 아닙니다".into());
        }
        std::fs::File::from(fd)
    };
    #[cfg(not(unix))]
    let file =
        std::fs::File::open(zip_path).map_err(|error| format!("백업 zip 열기 실패: {error}"))?;

    zip::ZipArchive::new(file).map_err(|error| format!("백업 zip 파싱 실패: {error}"))
}

fn validate_snapshot_entry(entry: &zip::read::ZipFile<'_>, relative: &Path) -> Result<(), String> {
    let top_level = relative
        .components()
        .next()
        .and_then(|component| match component {
            std::path::Component::Normal(name) => name.to_str(),
            _ => None,
        })
        .ok_or_else(|| "비어 있거나 안전하지 않은 zip 경로".to_string())?;
    if !IMPORT_NAMES.contains(&top_level) {
        return Err(format!(
            "허용되지 않은 snapshot 항목: {}",
            relative.display()
        ));
    }
    if top_level == "data"
        && relative != Path::new("data")
        && relative != Path::new("data/lifeops.db")
    {
        return Err(format!(
            "허용되지 않은 snapshot data 항목: {}",
            relative.display()
        ));
    }

    if let Some(mode) = entry.unix_mode() {
        let kind = mode & 0o170000;
        let expected = if entry.is_dir() { 0o040000 } else { 0o100000 };
        if kind != 0 && kind != expected {
            return Err(format!(
                "symlink 또는 특수 zip 엔트리 거부: {}",
                relative.display()
            ));
        }
    }
    Ok(())
}

fn validate_snapshot_entry_conflicts(entries: &HashMap<PathBuf, bool>) -> Result<(), String> {
    for (path, is_dir) in entries {
        let mut parent = path.parent();
        while let Some(ancestor) = parent {
            if entries
                .get(ancestor)
                .is_some_and(|ancestor_is_dir| !ancestor_is_dir)
            {
                return Err(format!("zip 파일/디렉터리 경로 충돌: {}", path.display()));
            }
            parent = ancestor.parent();
        }
        if !is_dir
            && entries
                .keys()
                .any(|candidate| candidate != path && candidate.starts_with(path))
        {
            return Err(format!("zip 파일/디렉터리 경로 충돌: {}", path.display()));
        }
    }
    Ok(())
}

/// 바인드 변경·복원 staging 후 앱을 재시작해 적용한다.
#[tauri::command]
fn relaunch_app(app: AppHandle) {
    app.restart();
}

struct ImportGuard;

impl ImportGuard {
    fn acquire() -> Result<Self, String> {
        IMPORT_IN_PROGRESS
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .map_err(|_| "다른 데이터 가져오기가 진행 중입니다".to_string())?;
        Ok(Self)
    }
}

impl Drop for ImportGuard {
    fn drop(&mut self) {
        IMPORT_IN_PROGRESS.store(false, Ordering::Release);
    }
}

async fn stage_import(source: &Path, data_dir: &Path) -> Result<(), String> {
    let source_meta = std::fs::symlink_metadata(source)
        .map_err(|error| format!("가져올 경로 확인 실패: {error}"))?;
    if source_meta.file_type().is_symlink() || !source_meta.is_dir() {
        return Err("가져올 경로는 symlink가 아닌 디렉터리여야 합니다".into());
    }
    let source = source
        .canonicalize()
        .map_err(|error| format!("가져올 경로 정규화 실패: {error}"))?;
    std::fs::create_dir_all(data_dir)
        .map_err(|error| format!("데이터 디렉터리 생성 실패: {error}"))?;
    let destination = data_dir
        .canonicalize()
        .map_err(|error| format!("데이터 디렉터리 정규화 실패: {error}"))?;
    if source == destination || source.starts_with(&destination) || destination.starts_with(&source)
    {
        return Err("가져올 경로와 현재 데이터 디렉터리가 겹칩니다".into());
    }

    let pending = destination.join(PENDING_IMPORT_DIR);
    if std::fs::symlink_metadata(&pending).is_ok() {
        return Err("이미 적용 대기 중인 가져오기가 있습니다".into());
    }
    let staging = destination.join(format!("{IMPORT_STAGING_PREFIX}{}", import_id()));
    std::fs::create_dir(&staging)
        .map_err(|error| format!("가져오기 staging 생성 실패: {error}"))?;

    let result = async {
        let count = stage_import_contents(&source, &staging).await?;
        if count == 0 {
            return Err("schemas, views, data, categories.yaml 중 가져올 항목이 없습니다".into());
        }
        validate_final_candidate(&staging, &destination).await?;
        std::fs::write(staging.join(IMPORT_READY_MARKER), b"ready\n")
            .map_err(|error| format!("가져오기 준비 marker 기록 실패: {error}"))?;
        sync_directory(&staging)?;
        std::fs::rename(&staging, &pending)
            .map_err(|error| format!("완료된 가져오기 publish 실패: {error}"))?;
        sync_directory(&destination)?;
        Ok(())
    }
    .await;
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result
}

#[cfg(unix)]
async fn stage_import_contents(source: &Path, pending: &Path) -> Result<usize, String> {
    let root = rustix::fs::open(
        source,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::DIRECTORY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(|error| format!("source root no-follow open 실패: {error}"))?;
    let mut count = 0;
    for name in IMPORT_NAMES {
        let child = match rustix::fs::openat(
            &root,
            name,
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC
                | rustix::fs::OFlags::NONBLOCK,
            rustix::fs::Mode::empty(),
        ) {
            Ok(fd) => fd,
            Err(error) if error == rustix::io::Errno::NOENT => continue,
            Err(error) if error == rustix::io::Errno::LOOP => {
                return Err(format!("top-level symlink 거부: {name}"));
            }
            Err(error) => return Err(format!("top-level openat 실패({name}): {error}")),
        };
        let stat = rustix::fs::fstat(&child).map_err(|error| error.to_string())?;
        let kind = rustix::fs::FileType::from_raw_mode(stat.st_mode);
        let to = pending.join(name);
        match name {
            "schemas" | "views" if kind.is_dir() => copy_tree_from_fd(&child, &to, &[])?,
            "categories.yaml" if kind.is_file() => copy_regular_fd(child, &to)?,
            "data" if kind.is_dir() => {
                copy_tree_from_fd(
                    &child,
                    &to,
                    &[
                        b"lifeops.db",
                        b"lifeops.db-wal",
                        b"lifeops.db-shm",
                        b"lifeops.db-journal",
                    ],
                )?;
                let database = source.join("data/lifeops.db");
                snapshot_sqlite(&database, &to.join("lifeops.db")).await?;
            }
            _ => return Err(format!("{name}의 파일 타입이 올바르지 않습니다")),
        }
        count += 1;
    }
    Ok(count)
}

#[cfg(not(unix))]
async fn stage_import_contents(source: &Path, pending: &Path) -> Result<usize, String> {
    let mut count = 0;
    for name in IMPORT_NAMES {
        let from = source.join(name);
        let metadata = match std::fs::symlink_metadata(&from) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("{} 확인 실패: {error}", from.display())),
        };
        if metadata.file_type().is_symlink() {
            return Err(format!("symlink는 가져올 수 없습니다: {}", from.display()));
        }
        let to = pending.join(name);
        if name == "data" {
            if !metadata.is_dir() {
                return Err(format!("data는 디렉터리여야 합니다: {}", from.display()));
            }
            copy_data_dir(&from, &to).await?;
        } else if name == "categories.yaml" && metadata.is_file() {
            copy_regular_file_no_follow(&from, &to)?;
        } else if (name == "schemas" || name == "views") && metadata.is_dir() {
            copy_tree_without_symlinks(&from, &to)?;
        } else {
            return Err(format!("{name}의 파일 타입이 올바르지 않습니다"));
        }
        count += 1;
    }
    Ok(count)
}

#[cfg(unix)]
fn copy_regular_fd(fd: std::os::fd::OwnedFd, destination: &Path) -> Result<(), String> {
    let stat = rustix::fs::fstat(&fd).map_err(|error| error.to_string())?;
    if !rustix::fs::FileType::from_raw_mode(stat.st_mode).is_file() {
        return Err("source fd가 일반 파일이 아닙니다".into());
    }
    let mut input = std::fs::File::from(fd);
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| format!("{} 생성 실패: {error}", destination.display()))?;
    std::io::copy(&mut input, &mut output)
        .and_then(|_| output.sync_all())
        .map_err(|error| format!("{} fd 복사 실패: {error}", destination.display()))?;
    Ok(())
}

#[cfg(not(unix))]
fn copy_regular_file_no_follow(source: &Path, destination: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        let fd = rustix::fs::open(
            source,
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC
                | rustix::fs::OFlags::NONBLOCK,
            rustix::fs::Mode::empty(),
        )
        .map_err(|error| format!("{} no-follow open 실패: {error}", source.display()))?;
        let stat = rustix::fs::fstat(&fd).map_err(|error| error.to_string())?;
        if !rustix::fs::FileType::from_raw_mode(stat.st_mode).is_file() {
            return Err(format!("source 일반 파일이 아님: {}", source.display()));
        }
        let mut input = std::fs::File::from(fd);
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)
            .map_err(|error| format!("{} 생성 실패: {error}", destination.display()))?;
        std::io::copy(&mut input, &mut output)
            .and_then(|_| output.sync_all())
            .map_err(|error| format!("{} fd 복사 실패: {error}", source.display()))?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        std::fs::copy(source, destination)
            .map(|_| ())
            .map_err(|error| format!("{} 복사 실패: {error}", source.display()))
    }
}

#[cfg(not(unix))]
async fn copy_data_dir(source: &Path, destination: &Path) -> Result<(), String> {
    let database = source.join("lifeops.db");
    let database_metadata = std::fs::symlink_metadata(&database)
        .map_err(|error| format!("data/lifeops.db 확인 실패: {error}"))?;
    if database_metadata.file_type().is_symlink() || !database_metadata.is_file() {
        return Err("data/lifeops.db는 symlink가 아닌 일반 파일이어야 합니다".into());
    }
    #[cfg(unix)]
    {
        let source_fd = rustix::fs::open(
            source,
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::DIRECTORY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .map_err(|error| format!("data no-follow open 실패: {error}"))?;
        copy_tree_from_fd(
            &source_fd,
            destination,
            &[
                b"lifeops.db",
                b"lifeops.db-wal",
                b"lifeops.db-shm",
                b"lifeops.db-journal",
            ],
        )?;
        snapshot_sqlite(&database, &destination.join("lifeops.db")).await?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir(destination)
            .map_err(|error| format!("{} 생성 실패: {error}", destination.display()))?;
        for entry in std::fs::read_dir(source)
            .map_err(|error| format!("{} 읽기 실패: {error}", source.display()))?
        {
            let entry = entry.map_err(|error| format!("data 항목 읽기 실패: {error}"))?;
            let from = entry.path();
            let to = destination.join(entry.file_name());
            let metadata = std::fs::symlink_metadata(&from)
                .map_err(|error| format!("{} 확인 실패: {error}", from.display()))?;
            if metadata.file_type().is_symlink() {
                return Err(format!("symlink는 가져올 수 없습니다: {}", from.display()));
            } else if matches!(
                entry.file_name().to_str(),
                Some("lifeops.db-wal" | "lifeops.db-shm" | "lifeops.db-journal")
            ) {
                continue;
            }
            if entry.file_name() == "lifeops.db" && metadata.is_file() {
                snapshot_sqlite(&from, &to).await?;
            } else if metadata.is_dir() {
                copy_tree_without_symlinks(&from, &to)?;
            } else if metadata.is_file() {
                std::fs::copy(&from, &to)
                    .map_err(|error| format!("{} 복사 실패: {error}", from.display()))?;
            } else {
                return Err(format!("일반 파일/디렉터리가 아님: {}", from.display()));
            }
        }
        Ok(())
    }
}

async fn validate_final_candidate(staging: &Path, destination: &Path) -> Result<(), String> {
    let schemas_path = candidate_path(staging, destination, "schemas");
    let views_path = candidate_path(staging, destination, "views");
    let categories_path = candidate_path(staging, destination, "categories.yaml");
    let database_path = candidate_path(staging, destination, "data").join("lifeops.db");

    for path in [&schemas_path, &views_path, &categories_path, &database_path] {
        if std::fs::symlink_metadata(path).is_ok() {
            validate_tree_without_symlinks(path)?;
        }
    }

    let schemas = lifeops_core::schema::SchemaSet::load_dir(&schemas_path)
        .map_err(|error| format!("최종 schema 검증 실패: {error}"))?;
    let _categories = lifeops_core::schema::load_categories(&categories_path)
        .map_err(|error| format!("최종 category 검증 실패: {error}"))?;

    let pages = lifeops_core::view::PageSet::load_dir(&views_path)
        .map_err(|error| format!("최종 view 검증 실패: {error}"))?;
    for page in pages.all() {
        for block in &page.blocks {
            if schemas.get(&block.source).is_none() {
                return Err(format!(
                    "page {}의 block source가 존재하지 않습니다: {}",
                    page.page, block.source
                ));
            }
        }
    }

    validate_candidate_database(&database_path, &schemas).await?;
    validate_candidate_pages(&database_path, &schemas, &pages).await
}

fn candidate_path(staging: &Path, destination: &Path, name: &str) -> PathBuf {
    let staged = staging.join(name);
    if staged.exists() {
        staged
    } else {
        destination.join(name)
    }
}

async fn validate_candidate_database(
    database: &Path,
    schemas: &lifeops_core::schema::SchemaSet,
) -> Result<(), String> {
    if !database.is_file() {
        return Err(format!(
            "최종 LifeOps DB가 없습니다: {}",
            database.display()
        ));
    }
    let metadata = std::fs::symlink_metadata(database)
        .map_err(|error| format!("최종 DB 확인 실패: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("최종 DB가 symlink가 아닌 일반 파일이어야 합니다".into());
    }
    let options = SqliteConnectOptions::new()
        .filename(database)
        .read_only(true)
        .disable_statement_logging();
    let mut connection = sqlx::SqliteConnection::connect_with(&options)
        .await
        .map_err(|error| format!("최종 DB 연결 실패: {error}"))?;
    let required_tables: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('entities', 'refs')",
    )
    .fetch_one(&mut connection)
    .await
    .map_err(|error| format!("최종 DB schema 확인 실패: {error}"))?;
    if required_tables != 2 {
        return Err("최종 DB에 entities/refs 테이블이 모두 필요합니다".into());
    }
    validate_table_columns(
        &mut connection,
        "entities",
        &["id", "type", "data", "created_at", "updated_at"],
    )
    .await?;
    validate_table_constraints(
        &mut connection,
        "entities",
        &["type", "data", "created_at", "updated_at"],
        &["id"],
    )
    .await?;
    validate_table_constraints(
        &mut connection,
        "refs",
        &["from_id", "to_id", "field_name"],
        &["from_id", "to_id", "field_name"],
    )
    .await?;
    validate_table_columns(&mut connection, "refs", &["from_id", "to_id", "field_name"]).await?;
    let rows: Vec<(String, String, String, String, String)> =
        sqlx::query_as("SELECT id, type, data, created_at, updated_at FROM entities")
            .fetch_all(&mut connection)
            .await
            .map_err(|error| format!("최종 DB entity 조회 실패: {error}"))?;
    for (id, entity_type, raw_data, _created_at, _updated_at) in rows {
        let schema = schemas
            .get(&entity_type)
            .ok_or_else(|| format!("entity {id}의 type이 최종 schema에 없습니다: {entity_type}"))?;
        let data = serde_json::from_str::<serde_json::Value>(&raw_data)
            .map_err(|error| format!("entity {id} data JSON 파싱 실패: {error}"))?
            .as_object()
            .cloned()
            .ok_or_else(|| format!("entity {id} data가 JSON object가 아닙니다"))?;
        lifeops_core::entity::validate_entity(schema, &data)
            .map_err(|error| format!("entity {id} 검증 실패: {error}"))?;
    }
    let dangling_refs: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM refs r LEFT JOIN entities f ON f.id = r.from_id LEFT JOIN entities t ON t.id = r.to_id WHERE f.id IS NULL OR t.id IS NULL",
    )
    .fetch_one(&mut connection)
    .await
    .map_err(|error| format!("최종 DB refs 검증 실패: {error}"))?;
    if dangling_refs != 0 {
        return Err(format!(
            "최종 DB에 dangling refs가 있습니다: {dangling_refs}"
        ));
    }
    Ok(())
}

async fn validate_table_constraints(
    connection: &mut sqlx::SqliteConnection,
    table: &str,
    not_null: &[&str],
    primary_key: &[&str],
) -> Result<(), String> {
    let rows: Vec<(i64, String, String, i64, Option<String>, i64)> =
        sqlx::query_as(&format!("PRAGMA table_info({table})"))
            .fetch_all(connection)
            .await
            .map_err(|error| format!("최종 DB {table} constraints 확인 실패: {error}"))?;
    for column in not_null {
        let row = rows
            .iter()
            .find(|row| row.1 == *column)
            .ok_or_else(|| format!("{table}.{column} 누락"))?;
        if row.3 != 1 {
            return Err(format!("최종 DB {table}.{column} NOT NULL 계약 누락"));
        }
    }
    for (index, column) in primary_key.iter().enumerate() {
        let row = rows
            .iter()
            .find(|row| row.1 == *column)
            .ok_or_else(|| format!("{table}.{column} 누락"))?;
        if row.5 != (index + 1) as i64 {
            return Err(format!("최종 DB {table}.{column} PRIMARY KEY 계약 불일치"));
        }
    }
    Ok(())
}

async fn validate_table_columns(
    connection: &mut sqlx::SqliteConnection,
    table: &str,
    required: &[&str],
) -> Result<(), String> {
    let query = format!("PRAGMA table_info({table})");
    let rows: Vec<(i64, String, String, i64, Option<String>, i64)> = sqlx::query_as(&query)
        .fetch_all(connection)
        .await
        .map_err(|error| format!("최종 DB {table} columns 확인 실패: {error}"))?;
    let actual: std::collections::HashSet<&str> = rows.iter().map(|row| row.1.as_str()).collect();
    let missing: Vec<&&str> = required
        .iter()
        .filter(|column| !actual.contains(**column))
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!("최종 DB {table} 필수 column 누락: {missing:?}"))
    }
}

async fn validate_candidate_pages(
    database: &Path,
    schemas: &lifeops_core::schema::SchemaSet,
    pages: &lifeops_core::view::PageSet,
) -> Result<(), String> {
    let parent = database
        .parent()
        .ok_or_else(|| "최종 DB parent가 없습니다".to_string())?;
    let validation_db = parent.join(format!(".import-validation-{}.db", import_id()));
    snapshot_sqlite(database, &validation_db).await?;
    let result = async {
        let store = lifeops_core::entity::EntityStore::open(&validation_db)
            .await
            .map_err(|error| format!("최종 DB EntityStore open 실패: {error}"))?;
        for page in pages.all() {
            lifeops_core::view::run_page(&store, schemas, page)
                .await
                .map_err(|error| format!("page {} 실행 검증 실패: {error}", page.page))?;
        }
        Ok(())
    }
    .await;
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", validation_db.display()));
    }
    result
}

async fn snapshot_sqlite(source: &Path, destination: &Path) -> Result<(), String> {
    #[cfg(unix)]
    let (source_guard, source_identity) = {
        use std::os::unix::fs::MetadataExt;
        let before = std::fs::symlink_metadata(source)
            .map_err(|error| format!("SQLite source lstat 실패: {error}"))?;
        if before.file_type().is_symlink() || !before.is_file() {
            return Err("SQLite source는 symlink가 아닌 일반 파일이어야 합니다".into());
        }
        let guard = rustix::fs::open(
            source,
            rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .map_err(|error| format!("SQLite source no-follow open 실패: {error}"))?;
        let opened = rustix::fs::fstat(&guard)
            .map_err(|error| format!("SQLite source fstat 실패: {error}"))?;
        let identity = (before.dev(), before.ino());
        if (opened.st_dev as u64, opened.st_ino as u64) != identity {
            return Err("SQLite source가 open 중 교체되었습니다".into());
        }
        (guard, identity)
    };
    // SQLite가 열린 source의 WAL sidecar까지 일관되게 읽으려면 원래 pathname이 필요해
    // `/dev/fd`로 열 수 없다. 대신 O_NOFOLLOW fd를 snapshot 전체 동안 유지하고 pathname의
    // open 전/후 dev+ino를 대조한다. 동일 로컬 사용자가 의도적으로 swap 후 같은 inode로
    // 복원하는 ABA 공격은 이 개인용 import UI의 threat model 밖이며, 정적 symlink와 우발적
    // 교체는 위/아래 검증에서 거부된다.
    let options = SqliteConnectOptions::new()
        .filename(source)
        .read_only(true)
        .disable_statement_logging();
    let mut connection = sqlx::SqliteConnection::connect_with(&options)
        .await
        .map_err(|error| format!("SQLite snapshot 연결 실패({}): {error}", source.display()))?;
    sqlx::query("VACUUM INTO ?")
        .bind(destination.to_string_lossy().as_ref())
        .execute(&mut connection)
        .await
        .map_err(|error| format!("SQLite snapshot 실패({}): {error}", source.display()))?;
    let snapshot_options = SqliteConnectOptions::new()
        .filename(destination)
        .read_only(true)
        .disable_statement_logging();
    let mut snapshot = sqlx::SqliteConnection::connect_with(&snapshot_options)
        .await
        .map_err(|error| format!("SQLite snapshot 재열기 실패: {error}"))?;
    let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&mut snapshot)
        .await
        .map_err(|error| format!("SQLite snapshot 무결성 검사 실패: {error}"))?;
    if integrity != "ok" {
        return Err(format!("SQLite snapshot 무결성 검사 실패: {integrity}"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let after = std::fs::symlink_metadata(source)
            .map_err(|error| format!("SQLite source 사후 lstat 실패: {error}"))?;
        if after.file_type().is_symlink()
            || !after.is_file()
            || (after.dev(), after.ino()) != source_identity
        {
            return Err("SQLite source가 snapshot 중 교체되었습니다".into());
        }
        drop(source_guard);
    }
    Ok(())
}

#[cfg(not(unix))]
fn copy_tree_without_symlinks(source: &Path, destination: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        let source_fd = rustix::fs::open(
            source,
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::DIRECTORY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .map_err(|error| format!("{} no-follow open 실패: {error}", source.display()))?;
        copy_tree_from_fd(&source_fd, destination, &[])
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir(destination)
            .map_err(|error| format!("{} 생성 실패: {error}", destination.display()))?;
        for entry in std::fs::read_dir(source)
            .map_err(|error| format!("{} 읽기 실패: {error}", source.display()))?
        {
            let entry = entry.map_err(|error| format!("디렉터리 항목 읽기 실패: {error}"))?;
            let from = entry.path();
            let to = destination.join(entry.file_name());
            let metadata = std::fs::symlink_metadata(&from)
                .map_err(|error| format!("{} 확인 실패: {error}", from.display()))?;
            if metadata.file_type().is_symlink() {
                return Err(format!("symlink는 가져올 수 없습니다: {}", from.display()));
            } else if metadata.is_dir() {
                copy_tree_without_symlinks(&from, &to)?;
            } else if metadata.is_file() {
                std::fs::copy(&from, &to)
                    .map_err(|error| format!("{} 복사 실패: {error}", from.display()))?;
            } else {
                return Err(format!("일반 파일/디렉터리가 아님: {}", from.display()));
            }
        }
        Ok(())
    }
}

#[cfg(unix)]
fn copy_tree_from_fd<Fd: std::os::fd::AsFd>(
    source_fd: Fd,
    destination: &Path,
    skip: &[&[u8]],
) -> Result<(), String> {
    use std::os::unix::ffi::OsStrExt;

    std::fs::create_dir(destination)
        .map_err(|error| format!("{} 생성 실패: {error}", destination.display()))?;
    let entries = rustix::fs::Dir::read_from(&source_fd)
        .map_err(|error| format!("source fd read 실패: {error}"))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("source fd entry 실패: {error}"))?;
        let name = entry.file_name();
        if name.to_bytes() == b"." || name.to_bytes() == b".." {
            continue;
        }
        if skip.contains(&name.to_bytes()) {
            continue;
        }
        let child = rustix::fs::openat(
            &source_fd,
            name,
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC
                | rustix::fs::OFlags::NONBLOCK,
            rustix::fs::Mode::empty(),
        )
        .map_err(|error| {
            if error == rustix::io::Errno::LOOP {
                format!("source child symlink 거부: {}", name.to_string_lossy())
            } else {
                format!("source child no-follow open 실패: {error}")
            }
        })?;
        let stat = rustix::fs::fstat(&child)
            .map_err(|error| format!("source child fstat 실패: {error}"))?;
        let file_type = rustix::fs::FileType::from_raw_mode(stat.st_mode);
        let target = destination.join(std::ffi::OsStr::from_bytes(name.to_bytes()));
        if file_type.is_dir() {
            copy_tree_from_fd(&child, &target, &[])?;
        } else if file_type.is_file() {
            let mut input = std::fs::File::from(child);
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)
                .map_err(|error| format!("{} 생성 실패: {error}", target.display()))?;
            std::io::copy(&mut input, &mut output)
                .and_then(|_| output.sync_all())
                .map_err(|error| format!("{} fd 복사 실패: {error}", target.display()))?;
        } else {
            return Err(format!("source 특수 파일 거부: {}", target.display()));
        }
    }
    sync_directory(destination)
}

async fn apply_pending_import(data_dir: &Path) -> Result<bool, String> {
    let pending = data_dir.join(PENDING_IMPORT_DIR);
    match std::fs::symlink_metadata(&pending) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(format!("가져오기 staging 확인 실패: {error}")),
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err("가져오기 staging이 안전한 디렉터리가 아닙니다".into());
        }
        Ok(_) => {}
    }
    let ready = pending.join(IMPORT_READY_MARKER);
    let ready_meta = match std::fs::symlink_metadata(&ready) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let is_empty = std::fs::read_dir(&pending)
                .map_err(|read_error| format!("staging residue 확인 실패: {read_error}"))?
                .next()
                .is_none();
            if is_empty {
                // 성공 적용 뒤 ready 제거는 됐지만 빈 pending 제거만 실패한 residue.
                std::fs::remove_dir(&pending).map_err(|remove_error| {
                    format!("빈 staging residue 정리 실패: {remove_error}")
                })?;
                return Ok(true);
            }
            return Err("완료되지 않은 가져오기 staging입니다".into());
        }
        Err(error) => return Err(format!("가져오기 준비 marker 확인 실패: {error}")),
    };
    if ready_meta.file_type().is_symlink() || !ready_meta.is_file() {
        return Err("가져오기 준비 marker가 일반 파일이 아닙니다".into());
    }
    cleanup_manifest_temps(&pending)?;

    let manifest_path = pending.join(IMPORT_MANIFEST);
    let mut manifest = if manifest_path.exists() {
        read_manifest(&manifest_path)?
    } else {
        let items = IMPORT_NAMES
            .iter()
            .filter(|name| pending.join(name).exists())
            .map(|name| {
                let target = data_dir.join(name);
                let had_target = safe_existing_target(&target)?;
                Ok(ImportManifestItem {
                    name: (*name).to_string(),
                    had_target,
                    phase: ImportPhase::Staged,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        if items.is_empty() {
            cleanup_committed_pending(&pending, &ready);
            return Ok(true);
        }
        for item in &items {
            validate_tree_without_symlinks(&pending.join(&item.name))?;
        }
        validate_final_candidate(&pending, data_dir).await?;
        let manifest = ImportManifest {
            version: 1,
            backup_id: import_id(),
            items,
        };
        write_manifest(&manifest_path, &manifest)?;
        sync_directory(&pending)?;
        manifest
    };
    validate_manifest(&manifest)?;

    let apply_dirs = ApplyDirs::new(data_dir, &pending, &manifest.backup_id)?;

    for index in 0..manifest.items.len() {
        let name = manifest.items[index].name.clone();
        if manifest.items[index].phase == ImportPhase::Staged {
            if manifest.items[index].had_target {
                match (
                    apply_dirs.target_exists(&name)?,
                    apply_dirs.backup_exists(&name)?,
                ) {
                    (true, false) => {
                        apply_dirs.backup_target(&name)?;
                    }
                    (false, true) => {}
                    state => {
                        return Err(format!("{name} backup 재개 상태가 모호합니다: {state:?}"))
                    }
                }
            }
            manifest.items[index].phase = ImportPhase::BackedUp;
            write_manifest(&manifest_path, &manifest)?;
        }
        if manifest.items[index].phase == ImportPhase::BackedUp {
            let saved_exists = apply_dirs.backup_exists(&name)?;
            if saved_exists != manifest.items[index].had_target {
                return Err(format!(
                    "{name} backup 보존 상태가 manifest와 다릅니다: expected={}, actual={saved_exists}",
                    manifest.items[index].had_target
                ));
            }
            match (
                apply_dirs.staged_exists(&name)?,
                apply_dirs.target_exists(&name)?,
            ) {
                (true, false) => {
                    apply_dirs.promote(&name)?;
                }
                (false, true) => {}
                state => return Err(format!("{name} promote 재개 상태가 모호합니다: {state:?}")),
            }
            manifest.items[index].phase = ImportPhase::Promoted;
            write_manifest(&manifest_path, &manifest)?;
        }
        if manifest.items[index].phase == ImportPhase::Promoted {
            match (
                apply_dirs.staged_exists(&name)?,
                apply_dirs.target_exists(&name)?,
            ) {
                (false, true) => {}
                state => {
                    return Err(format!(
                        "{name} promoted 상태가 manifest와 다릅니다: {state:?}"
                    ))
                }
            }
        }
    }
    if manifest
        .items
        .iter()
        .any(|item| item.phase != ImportPhase::Promoted)
    {
        return Err("가져오기 manifest가 완료 상태가 아닙니다".into());
    }
    cleanup_committed_pending(&pending, &ready);
    Ok(true)
}

fn cleanup_committed_pending(pending: &Path, ready: &Path) {
    if let Err(error) = cleanup_manifest_temps(pending) {
        tracing::warn!(%error, path = %pending.display(), "manifest temp 정리 실패");
    }
    if let Err(error) = std::fs::remove_file(pending.join(IMPORT_MANIFEST)) {
        if error.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(%error, path = %pending.display(), "적용 manifest 정리 실패");
        }
    }
    if let Err(error) = sync_directory(pending) {
        tracing::warn!(%error, path = %pending.display(), "manifest cleanup fsync 실패");
    }
    if let Err(error) = std::fs::remove_file(ready) {
        tracing::warn!(%error, path = %ready.display(), "적용 완료 marker 정리 실패");
    }
    if let Err(error) = sync_directory(pending) {
        tracing::warn!(%error, path = %pending.display(), "commit marker cleanup fsync 실패");
    }
    if let Err(error) = std::fs::remove_dir(pending) {
        tracing::warn!(%error, path = %pending.display(), "적용 완료 staging 정리 실패");
    }
}

fn cleanup_manifest_temps(pending: &Path) -> Result<(), String> {
    let prefix = format!(".{IMPORT_MANIFEST}.tmp-");
    for entry in
        std::fs::read_dir(pending).map_err(|error| format!("manifest temp 목록 실패: {error}"))?
    {
        let entry = entry.map_err(|error| format!("manifest temp 항목 실패: {error}"))?;
        if !entry.file_name().to_string_lossy().starts_with(&prefix) {
            continue;
        }
        let metadata = std::fs::symlink_metadata(entry.path())
            .map_err(|error| format!("manifest temp 확인 실패: {error}"))?;
        if metadata.is_dir() {
            return Err("manifest temp 위치에 디렉터리가 있습니다".into());
        }
        std::fs::remove_file(entry.path())
            .map_err(|error| format!("manifest temp 제거 실패: {error}"))?;
    }
    sync_directory(pending)
}

fn read_manifest(path: &Path) -> Result<ImportManifest, String> {
    let metadata =
        std::fs::symlink_metadata(path).map_err(|error| format!("manifest 확인 실패: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("manifest가 일반 파일이 아닙니다".into());
    }
    let bytes = std::fs::read(path).map_err(|error| format!("manifest 읽기 실패: {error}"))?;
    serde_json::from_slice(&bytes).map_err(|error| format!("manifest 파싱 실패: {error}"))
}

fn write_manifest(path: &Path, manifest: &ImportManifest) -> Result<(), String> {
    use std::io::Write;
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err("기존 manifest가 일반 파일이 아닙니다".into());
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("기존 manifest 확인 실패: {error}")),
    }
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "manifest 파일명이 유효하지 않습니다".to_string())?;
    let temp = path.with_file_name(format!(".{file_name}.tmp-{}", import_id()));
    let bytes = serde_json::to_vec(manifest).map_err(|error| error.to_string())?;
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp)
        .map_err(|error| format!("manifest temp 열기 실패: {error}"))?;
    if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = std::fs::remove_file(&temp);
        return Err(format!("manifest fsync 실패: {error}"));
    }
    drop(file);
    if let Err(error) = std::fs::rename(&temp, path) {
        let _ = std::fs::remove_file(&temp);
        return Err(format!("manifest publish 실패: {error}"));
    }
    sync_directory(path.parent().expect("manifest parent"))
}

fn validate_manifest(manifest: &ImportManifest) -> Result<(), String> {
    if manifest.version != 1 || manifest.items.is_empty() {
        return Err("지원하지 않거나 비어 있는 import manifest입니다".into());
    }
    if manifest.backup_id.is_empty()
        || Path::new(&manifest.backup_id)
            .file_name()
            .and_then(|v| v.to_str())
            != Some(manifest.backup_id.as_str())
    {
        return Err("manifest backup id가 안전하지 않습니다".into());
    }
    let mut seen = std::collections::HashSet::new();
    for item in &manifest.items {
        if !IMPORT_NAMES.contains(&item.name.as_str()) || !seen.insert(item.name.as_str()) {
            return Err(format!(
                "manifest payload가 안전하지 않습니다: {}",
                item.name
            ));
        }
    }
    Ok(())
}

fn safe_existing_target(path: &Path) -> Result<bool, String> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(format!(
            "symlink 대상은 허용되지 않습니다: {}",
            path.display()
        )),
        Ok(metadata) if metadata.is_file() || metadata.is_dir() => Ok(true),
        Ok(_) => Err(format!(
            "특수 파일 대상은 허용되지 않습니다: {}",
            path.display()
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!("{} 확인 실패: {error}", path.display())),
    }
}

#[cfg(not(unix))]
fn ensure_safe_directory(path: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir(path)
                .map_err(|create_error| format!("{} 생성 실패: {create_error}", path.display()))?;
        }
        Err(error) => return Err(format!("{} 확인 실패: {error}", path.display())),
        Ok(_) => {}
    }
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("{} 재검증 실패: {error}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!(
            "안전한 실제 디렉터리가 아닙니다: {}",
            path.display()
        ));
    }
    Ok(())
}

#[cfg(unix)]
struct ApplyDirs {
    data: std::os::fd::OwnedFd,
    pending: std::os::fd::OwnedFd,
    backup: std::os::fd::OwnedFd,
}

#[cfg(unix)]
impl ApplyDirs {
    fn new(data_dir: &Path, _pending: &Path, backup_id: &str) -> Result<Self, String> {
        let data = open_directory_no_follow(data_dir)?;
        let pending = open_child_directory(&data, PENDING_IMPORT_DIR, false)?;
        let backups = open_child_directory(&data, IMPORT_BACKUPS_DIR, true)?;
        let backup = open_child_directory(&backups, backup_id, true)?;
        rustix::fs::fsync(&data).map_err(|error| error.to_string())?;
        rustix::fs::fsync(&backups).map_err(|error| error.to_string())?;
        Ok(Self {
            data,
            pending,
            backup,
        })
    }

    fn target_exists(&self, name: &str) -> Result<bool, String> {
        child_exists(&self.data, name)
    }

    fn staged_exists(&self, name: &str) -> Result<bool, String> {
        child_exists(&self.pending, name)
    }

    fn backup_exists(&self, name: &str) -> Result<bool, String> {
        child_exists(&self.backup, name)
    }

    fn backup_target(&self, name: &str) -> Result<(), String> {
        rustix::fs::renameat(&self.data, name, &self.backup, name)
            .map_err(|error| format!("기존 {name} fd-relative backup 실패: {error}"))?;
        rustix::fs::fsync(&self.data).map_err(|error| error.to_string())?;
        rustix::fs::fsync(&self.backup).map_err(|error| error.to_string())?;
        Ok(())
    }

    fn promote(&self, name: &str) -> Result<(), String> {
        rustix::fs::renameat(&self.pending, name, &self.data, name)
            .map_err(|error| format!("새 {name} fd-relative 적용 실패: {error}"))?;
        rustix::fs::fsync(&self.pending).map_err(|error| error.to_string())?;
        rustix::fs::fsync(&self.data).map_err(|error| error.to_string())?;
        Ok(())
    }
}

#[cfg(unix)]
fn open_directory_no_follow(path: &Path) -> Result<std::os::fd::OwnedFd, String> {
    rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::DIRECTORY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(|error| format!("{} directory fd open 실패: {error}", path.display()))
}

#[cfg(unix)]
fn open_child_directory<Fd: std::os::fd::AsFd>(
    parent: Fd,
    name: &str,
    create: bool,
) -> Result<std::os::fd::OwnedFd, String> {
    let flags = rustix::fs::OFlags::RDONLY
        | rustix::fs::OFlags::DIRECTORY
        | rustix::fs::OFlags::NOFOLLOW
        | rustix::fs::OFlags::CLOEXEC;
    match rustix::fs::openat(&parent, name, flags, rustix::fs::Mode::empty()) {
        Ok(fd) => Ok(fd),
        Err(error) if create && error == rustix::io::Errno::NOENT => {
            rustix::fs::mkdirat(
                &parent,
                name,
                rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR | rustix::fs::Mode::XUSR,
            )
            .map_err(|mkdir_error| format!("{name} mkdirat 실패: {mkdir_error}"))?;
            let fd = rustix::fs::openat(&parent, name, flags, rustix::fs::Mode::empty())
                .map_err(|open_error| format!("{name} 생성 후 openat 실패: {open_error}"))?;
            rustix::fs::fsync(&parent).map_err(|sync_error| sync_error.to_string())?;
            Ok(fd)
        }
        Err(error) => Err(format!("{name} no-follow directory openat 실패: {error}")),
    }
}

#[cfg(unix)]
fn child_exists<Fd: std::os::fd::AsFd>(parent: Fd, name: &str) -> Result<bool, String> {
    match rustix::fs::statat(&parent, name, rustix::fs::AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) => {
            let kind = rustix::fs::FileType::from_raw_mode(stat.st_mode);
            if kind.is_symlink() {
                Err(format!("fd-relative child symlink 거부: {name}"))
            } else if kind.is_file() || kind.is_dir() {
                Ok(true)
            } else {
                Err(format!("fd-relative child 특수 파일 거부: {name}"))
            }
        }
        Err(error) if error == rustix::io::Errno::NOENT => Ok(false),
        Err(error) => Err(format!("fd-relative child 확인 실패({name}): {error}")),
    }
}

#[cfg(not(unix))]
struct ApplyDirs {
    data: PathBuf,
    pending: PathBuf,
    backup: PathBuf,
}

#[cfg(not(unix))]
impl ApplyDirs {
    fn new(data: &Path, pending: &Path, backup_id: &str) -> Result<Self, String> {
        let backups = data.join(IMPORT_BACKUPS_DIR);
        ensure_safe_directory(&backups)?;
        let backup = backups.join(backup_id);
        ensure_safe_directory(&backup)?;
        Ok(Self {
            data: data.into(),
            pending: pending.into(),
            backup,
        })
    }
    fn target_exists(&self, name: &str) -> Result<bool, String> {
        safe_existing_target(&self.data.join(name))
    }
    fn staged_exists(&self, name: &str) -> Result<bool, String> {
        safe_existing_target(&self.pending.join(name))
    }
    fn backup_exists(&self, name: &str) -> Result<bool, String> {
        safe_existing_target(&self.backup.join(name))
    }
    fn backup_target(&self, name: &str) -> Result<(), String> {
        std::fs::rename(self.data.join(name), self.backup.join(name)).map_err(|e| e.to_string())
    }
    fn promote(&self, name: &str) -> Result<(), String> {
        std::fs::rename(self.pending.join(name), self.data.join(name)).map_err(|e| e.to_string())
    }
}

fn sync_directory(path: &Path) -> Result<(), String> {
    std::fs::File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|error| format!("directory fsync 실패({}): {error}", path.display()))
}

fn validate_tree_without_symlinks(path: &Path) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("{} 확인 실패: {error}", path.display()))?;
    if metadata.file_type().is_symlink() {
        return Err(format!("staging symlink 거부: {}", path.display()));
    }
    if metadata.is_dir() {
        for entry in std::fs::read_dir(path)
            .map_err(|error| format!("{} 읽기 실패: {error}", path.display()))?
        {
            validate_tree_without_symlinks(&entry.map_err(|e| e.to_string())?.path())?;
        }
    } else if !metadata.is_file() {
        return Err(format!("staging 특수 파일 거부: {}", path.display()));
    }
    Ok(())
}

fn import_id() -> String {
    let sequence = NEXT_IMPORT_ID.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{nanos}-{}-{sequence}", std::process::id())
}

fn show_main_window(app: &AppHandle<Wry>) {
    if let Some(window) = app.get_webview_window("main") {
        if let Err(error) = window.show() {
            tracing::error!(%error, "main 창 표시 실패");
        }
        if let Err(error) = window.set_focus() {
            tracing::error!(%error, "main 창 포커스 실패");
        }
    }
}

fn tray_address_label(addr: std::net::SocketAddr) -> String {
    let addresses = if addr.ip().is_loopback() {
        Vec::new()
    } else {
        lifeops_server::routes::system::lan_addresses(addr.port())
    };
    tray_address_label_from(addr, addresses)
}

fn tray_address_label_from(addr: std::net::SocketAddr, addresses: Vec<String>) -> String {
    if addr.ip().is_loopback() {
        return format!("접속 범위: 내 기기에서만 · 포트 {}", addr.port());
    }
    addresses
        .into_iter()
        .next()
        .map(|url| format!("LAN 주소: {url}"))
        .unwrap_or_else(|| format!("LAN 주소: 포트 {} · 설정에서 확인", addr.port()))
}

fn initialize_default_autostart(app: &AppHandle<Wry>, data_dir: &Path) {
    let manager = app.autolaunch();
    if let Err(error) = initialize_autostart_once(
        data_dir,
        || manager.enable().map_err(|error| error.to_string()),
        || manager.disable().map_err(|error| error.to_string()),
    ) {
        tracing::warn!(%error, "자동시작 기본값 초기화 실패");
    }
}

fn initialize_autostart_once<E, D>(data_dir: &Path, enable: E, disable: D) -> Result<bool, String>
where
    E: FnOnce() -> Result<(), String>,
    D: FnOnce() -> Result<(), String>,
{
    let marker = autostart_marker(data_dir);
    if completed_autostart_marker(&marker)? {
        return Ok(false);
    }
    std::fs::create_dir_all(data_dir).map_err(|error| error.to_string())?;
    enable()?;

    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&marker)
    {
        Ok(file) => match file.sync_all() {
            Ok(()) => Ok(true),
            Err(error) => {
                let _ = std::fs::remove_file(&marker);
                rollback_autostart(disable, error.to_string())
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            match completed_autostart_marker(&marker) {
                Ok(true) => Ok(true),
                Ok(false) => {
                    rollback_autostart(disable, "동시에 생성된 자동시작 marker가 사라짐".into())
                }
                Err(marker_error) => rollback_autostart(disable, marker_error),
            }
        }
        Err(error) => rollback_autostart(disable, error.to_string()),
    }
}

fn completed_autostart_marker(marker: &Path) -> Result<bool, String> {
    match std::fs::symlink_metadata(marker) {
        Ok(metadata) if metadata.file_type().is_file() => Ok(true),
        Ok(metadata) => Err(format!(
            "자동시작 marker가 일반 파일이 아님: {} ({:?})",
            marker.display(),
            metadata.file_type()
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!(
            "자동시작 marker 확인 실패({}): {error}",
            marker.display()
        )),
    }
}

fn rollback_autostart<D>(disable: D, marker_error: String) -> Result<bool, String>
where
    D: FnOnce() -> Result<(), String>,
{
    match disable() {
        Ok(()) => Err(format!("marker 기록 실패 후 자동시작 롤백: {marker_error}")),
        Err(rollback_error) => Err(format!(
            "marker 기록 실패: {marker_error}; 자동시작 롤백도 실패: {rollback_error}"
        )),
    }
}

fn autostart_marker(data_dir: &Path) -> PathBuf {
    data_dir.join(AUTOSTART_INIT_MARKER)
}

fn startup_failure_url(error: &str, logs_dir: &Path) -> String {
    let document = format!(
        "<meta charset=\"utf-8\"><h1>LifeOps 시작 실패</h1><pre>{}</pre><p>로그 위치: {}</p>",
        escape_html(error),
        escape_html(&logs_dir.display().to_string())
    );
    format!(
        "data:text/html;charset=utf-8,{}",
        utf8_percent_encode(&document, NON_ALPHANUMERIC)
    )
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn parse_target_url(target: &str, context: &str) -> Option<Url> {
    match target.parse::<Url>() {
        Ok(url) => Some(url),
        Err(error) => {
            tracing::error!(%error, %context, "Tauri 이동 URL 파싱 실패");
            None
        }
    }
}

fn navigate_when_ready(window: &WebviewWindow, target: Url) {
    if let Err(error) = window.navigate(target) {
        tracing::error!(%error, "Tauri webview URL 이동 실패");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use percent_encoding::percent_decode_str;
    use std::cell::Cell;
    use std::io::Write;

    fn make_snapshot_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = std::fs::File::create(path).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        for (name, contents) in entries {
            if name.ends_with('/') {
                archive
                    .add_directory(*name, zip::write::SimpleFileOptions::default())
                    .unwrap();
            } else {
                archive
                    .start_file(*name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                archive.write_all(contents).unwrap();
            }
        }
        archive.finish().unwrap();
    }

    #[test]
    fn unpack_snapshot은_import_layout을_그대로_푼다() {
        let root = tempfile::tempdir().unwrap();
        let snapshot = root.path().join("snapshot.zip");
        make_snapshot_zip(
            &snapshot,
            &[
                ("data/", b""),
                ("data/lifeops.db", b"DB"),
                ("schemas/", b""),
                ("schemas/item.yaml", b"type: item"),
                ("views/", b""),
                ("views/home.yaml", b"page: home"),
                ("categories.yaml", b"categories: []"),
            ],
        );
        let output = root.path().join("output");
        std::fs::create_dir(&output).unwrap();

        unpack_snapshot(&snapshot, &output).unwrap();

        assert_eq!(
            std::fs::read(output.join("data/lifeops.db")).unwrap(),
            b"DB"
        );
        assert_eq!(
            std::fs::read(output.join("schemas/item.yaml")).unwrap(),
            b"type: item"
        );
        assert!(output.join("views/home.yaml").is_file());
        assert!(output.join("categories.yaml").is_file());
    }

    #[test]
    fn unpack_snapshot은_zip_slip과_허용되지_않은_layout을_쓰기전에_거부한다() {
        for entry in ["../escape.txt", "/absolute.txt", "config.json"] {
            let root = tempfile::tempdir().unwrap();
            let snapshot = root.path().join("snapshot.zip");
            make_snapshot_zip(
                &snapshot,
                &[
                    ("data/", b""),
                    ("data/lifeops.db", b"DB"),
                    ("schemas/", b""),
                    ("views/", b""),
                    ("categories.yaml", b"categories: []"),
                    (entry, b"unsafe"),
                ],
            );
            let output = root.path().join("output");
            std::fs::create_dir(&output).unwrap();

            assert!(unpack_snapshot(&snapshot, &output).is_err());
            assert!(std::fs::read_dir(&output).unwrap().next().is_none());
            assert!(!root.path().join("escape.txt").exists());
        }
    }

    #[test]
    fn unpack_snapshot은_symlink_entry를_쓰기전에_거부한다() {
        let root = tempfile::tempdir().unwrap();
        let snapshot = root.path().join("snapshot.zip");
        let file = std::fs::File::create(&snapshot).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        archive
            .add_directory("data/", zip::write::SimpleFileOptions::default())
            .unwrap();
        archive
            .start_file("data/lifeops.db", zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(b"DB").unwrap();
        archive
            .add_directory("schemas/", zip::write::SimpleFileOptions::default())
            .unwrap();
        archive
            .add_directory("views/", zip::write::SimpleFileOptions::default())
            .unwrap();
        archive
            .start_file("categories.yaml", zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(b"categories: []").unwrap();
        archive
            .add_symlink(
                "schemas/escape",
                "../../outside",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        archive.finish().unwrap();
        let output = root.path().join("output");
        std::fs::create_dir(&output).unwrap();

        let error = unpack_snapshot(&snapshot, &output).unwrap_err();

        assert!(error.contains("symlink") || error.contains("특수"));
        assert!(std::fs::read_dir(&output).unwrap().next().is_none());
    }

    #[test]
    fn unpack_snapshot은_필수_database와_충돌없는_layout을_요구한다() {
        let root = tempfile::tempdir().unwrap();
        let missing_db = root.path().join("missing-db.zip");
        make_snapshot_zip(
            &missing_db,
            &[
                ("data/", b""),
                ("schemas/", b""),
                ("schemas/item.yaml", b"type: item"),
                ("views/", b""),
                ("categories.yaml", b"categories: []"),
            ],
        );
        let output = root.path().join("output");
        std::fs::create_dir(&output).unwrap();
        assert!(unpack_snapshot(&missing_db, &output)
            .unwrap_err()
            .contains("data/lifeops.db"));

        let conflict = root.path().join("conflict.zip");
        make_snapshot_zip(
            &conflict,
            &[
                ("data/", b""),
                ("data/lifeops.db", b"DB"),
                ("schemas/", b""),
                ("schemas/item", b"item"),
                ("schemas/item/child", b"child"),
                ("views/", b""),
                ("categories.yaml", b"categories: []"),
            ],
        );
        assert!(unpack_snapshot(&conflict, &output)
            .unwrap_err()
            .contains("충돌"));
        assert!(std::fs::read_dir(&output).unwrap().next().is_none());
    }

    #[test]
    fn unpack_snapshot은_partial_snapshot을_쓰기전에_거부한다() {
        let root = tempfile::tempdir().unwrap();
        let snapshot = root.path().join("partial.zip");
        make_snapshot_zip(
            &snapshot,
            &[
                ("data/", b""),
                ("data/lifeops.db", b"DB"),
                ("schemas/", b""),
                ("categories.yaml", b"categories: []"),
            ],
        );
        let output = root.path().join("output");
        std::fs::create_dir(&output).unwrap();

        let error = unpack_snapshot(&snapshot, &output).unwrap_err();

        assert!(error.contains("views"));
        assert!(std::fs::read_dir(&output).unwrap().next().is_none());
    }

    #[test]
    fn unpack_snapshot은_database외_data항목을_쓰기전에_거부한다() {
        for extra in ["data/extra", "data/nested/extra"] {
            let root = tempfile::tempdir().unwrap();
            let snapshot = root.path().join("extra-data.zip");
            make_snapshot_zip(
                &snapshot,
                &[
                    ("data/", b""),
                    ("data/lifeops.db", b"DB"),
                    ("schemas/", b""),
                    ("views/", b""),
                    ("categories.yaml", b"categories: []"),
                    (extra, b"unexpected"),
                ],
            );
            let output = root.path().join("output");
            std::fs::create_dir(&output).unwrap();

            let error = unpack_snapshot(&snapshot, &output).unwrap_err();

            assert!(error.contains("data 항목"));
            assert!(std::fs::read_dir(&output).unwrap().next().is_none());
        }
    }

    #[test]
    fn unpack_snapshot_output은_기존_stage_import_layout과_호환된다() {
        let root = tempfile::tempdir().unwrap();
        let snapshot_source = root.path().join("snapshot-source");
        let destination = root.path().join("destination");
        write_test_config(&snapshot_source);
        write_test_config(&destination);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        drop(runtime.block_on(open_test_lifeops_db(&snapshot_source)));

        let database = std::fs::read(snapshot_source.join("data/lifeops.db")).unwrap();
        let categories = std::fs::read(snapshot_source.join("categories.yaml")).unwrap();
        let snapshot = root.path().join("snapshot.zip");
        make_snapshot_zip(
            &snapshot,
            &[
                ("data/", b""),
                ("data/lifeops.db", &database),
                ("schemas/", b""),
                ("views/", b""),
                ("categories.yaml", &categories),
            ],
        );
        let extracted = root.path().join("extracted");
        std::fs::create_dir(&extracted).unwrap();

        unpack_snapshot(&snapshot, &extracted).unwrap();
        runtime
            .block_on(stage_import(&extracted, &destination))
            .unwrap();

        assert!(destination
            .join(PENDING_IMPORT_DIR)
            .join(IMPORT_READY_MARKER)
            .is_file());
        assert!(destination
            .join(PENDING_IMPORT_DIR)
            .join("data/lifeops.db")
            .is_file());
        assert!(
            std::fs::read_dir(destination.join(PENDING_IMPORT_DIR).join("schemas"))
                .unwrap()
                .next()
                .is_none()
        );
        assert!(
            std::fs::read_dir(destination.join(PENDING_IMPORT_DIR).join("views"))
                .unwrap()
                .next()
                .is_none()
        );
    }

    #[test]
    fn server가_생성한_snapshot은_unpack_contract와_호환된다() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let paths = lifeops_server::resolve_paths(&source);
        lifeops_server::install_seed_if_empty(&paths).unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        drop(
            runtime
                .block_on(lifeops_core::entity::EntityStore::open(&paths.db_path))
                .unwrap(),
        );
        let snapshot = runtime
            .block_on(lifeops_server::backup::create_snapshot(
                &paths,
                &paths.backups_dir,
                7,
            ))
            .unwrap();
        let extracted = root.path().join("extracted");
        std::fs::create_dir(&extracted).unwrap();

        unpack_snapshot(&snapshot, &extracted).unwrap();

        assert!(extracted.join("data/lifeops.db").is_file());
        assert!(extracted.join("schemas").is_dir());
        assert!(extracted.join("views").is_dir());
        assert!(extracted.join("categories.yaml").is_file());
    }

    #[test]
    fn snapshot_budget은_엔트리수와_총비압축크기를_제한한다() {
        assert!(
            validate_snapshot_budget(MAX_SNAPSHOT_ENTRIES, MAX_SNAPSHOT_UNCOMPRESSED_BYTES).is_ok()
        );
        assert!(validate_snapshot_budget(MAX_SNAPSHOT_ENTRIES + 1, 0)
            .unwrap_err()
            .contains("엔트리 수"));
        assert!(
            validate_snapshot_budget(1, MAX_SNAPSHOT_UNCOMPRESSED_BYTES + 1)
                .unwrap_err()
                .contains("비압축 크기")
        );
        assert!(validate_snapshot_entry_budget(MAX_SNAPSHOT_ENTRY_BYTES + 1)
            .unwrap_err()
            .contains("개별 엔트리"));
    }

    #[test]
    fn unpack_snapshot은_cap이내_server_style_고압축_snapshot을_허용한다() {
        let root = tempfile::tempdir().unwrap();
        let snapshot = root.path().join("compressed-bomb.zip");
        let file = std::fs::File::create(&snapshot).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        archive
            .add_directory("data/", zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.start_file("data/lifeops.db", options).unwrap();
        archive.write_all(&vec![0; 1024 * 1024]).unwrap();
        archive
            .add_directory("schemas/", zip::write::SimpleFileOptions::default())
            .unwrap();
        archive
            .add_directory("views/", zip::write::SimpleFileOptions::default())
            .unwrap();
        archive
            .start_file("categories.yaml", zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(b"categories: []").unwrap();
        archive.finish().unwrap();
        let output = root.path().join("output");
        std::fs::create_dir(&output).unwrap();

        unpack_snapshot(&snapshot, &output).unwrap();

        assert_eq!(
            std::fs::metadata(output.join("data/lifeops.db"))
                .unwrap()
                .len(),
            1024 * 1024
        );
    }

    #[test]
    fn unpack_snapshot은_다수_엔트리_bomb을_거부한다() {
        let root = tempfile::tempdir().unwrap();
        let snapshot = root.path().join("many-entries.zip");
        let file = std::fs::File::create(&snapshot).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        archive
            .add_directory("data/", zip::write::SimpleFileOptions::default())
            .unwrap();
        archive
            .start_file("data/lifeops.db", zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(b"DB").unwrap();
        archive
            .add_directory("schemas/", zip::write::SimpleFileOptions::default())
            .unwrap();
        archive
            .add_directory("views/", zip::write::SimpleFileOptions::default())
            .unwrap();
        archive
            .start_file("categories.yaml", zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(b"categories: []").unwrap();
        for index in 0..=(MAX_SNAPSHOT_ENTRIES - 4) {
            archive
                .start_file(
                    format!("schemas/{index}.yaml"),
                    zip::write::SimpleFileOptions::default(),
                )
                .unwrap();
        }
        archive.finish().unwrap();
        let output = root.path().join("output");
        std::fs::create_dir(&output).unwrap();

        let error = unpack_snapshot(&snapshot, &output).unwrap_err();

        assert!(error.contains("엔트리 수"));
        assert!(std::fs::read_dir(&output).unwrap().next().is_none());
    }

    #[test]
    fn restore는_pending_import를_추출전에_lstat로_거부한다() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join(PENDING_IMPORT_DIR)).unwrap();

        assert!(ensure_no_pending_import(root.path())
            .unwrap_err()
            .contains("적용 대기"));
    }

    fn write_test_config(root: &Path) {
        std::fs::create_dir_all(root.join("schemas")).unwrap();
        std::fs::write(
            root.join("schemas/item.yaml"),
            "type: 항목\ncategory: 기타\nfields:\n  이름: { kind: text, required: true }\n",
        )
        .unwrap();
        std::fs::write(
            root.join("categories.yaml"),
            "categories:\n  - { name: 기타 }\n",
        )
        .unwrap();
    }

    async fn open_test_lifeops_db(root: &Path) -> lifeops_core::entity::EntityStore {
        std::fs::create_dir_all(root.join("data")).unwrap();
        lifeops_core::entity::EntityStore::open(&root.join("data/lifeops.db"))
            .await
            .unwrap()
    }

    #[test]
    fn html_escape는_스크립트_앰퍼샌드와_quotes를_이스케이프한다() {
        let escaped = escape_html("<script>alert(\"x\" & 'y')</script>");

        assert_eq!(
            escaped,
            "&lt;script&gt;alert(&quot;x&quot; &amp; &#39;y&#39;)&lt;/script&gt;"
        );
    }

    #[test]
    fn 실패_url은_utf8_payload를_인코딩하고_fragment를_만들지_않는다() {
        let error = "실패 #50% <script>alert(\"x\" & 'y')</script>";
        let logs = Path::new("/tmp/로그 경로/#50% & \"인용\"");
        let raw = startup_failure_url(error, logs);
        let url = raw.parse::<Url>().expect("유효한 data URL");
        let encoded = url.as_str().split_once(',').expect("data URL payload").1;
        let decoded = percent_decode_str(encoded)
            .decode_utf8()
            .expect("UTF-8 payload");

        assert_eq!(url.scheme(), "data");
        assert!(url.fragment().is_none());
        assert!(decoded.contains("실패 #50% &lt;script&gt;"));
        assert!(decoded.contains("&quot;x&quot; &amp; &#39;y&#39;"));
        assert!(decoded.contains("/tmp/로그 경로/#50% &amp; &quot;인용&quot;"));
        assert!(!decoded.contains("<script>"));
    }

    #[test]
    fn tray_label은_loopback이면_내기기전용을_표시한다() {
        assert_eq!(
            tray_address_label_from(
                "127.0.0.1:3012".parse().unwrap(),
                vec!["http://192.168.0.7:3012".into()]
            ),
            "접속 범위: 내 기기에서만 · 포트 3012"
        );
        assert_eq!(
            tray_address_label_from("[::1]:3013".parse().unwrap(), vec![]),
            "접속 범위: 내 기기에서만 · 포트 3013"
        );
    }

    #[test]
    fn tray_label은_lan이면_실제_주소를_우선하고_없으면_port를_안내한다() {
        assert_eq!(
            tray_address_label_from(
                "0.0.0.0:3012".parse().unwrap(),
                vec!["http://192.168.0.7:3012".into()]
            ),
            "LAN 주소: http://192.168.0.7:3012"
        );
        assert_eq!(
            tray_address_label_from("0.0.0.0:3012".parse().unwrap(), vec![]),
            "LAN 주소: 포트 3012 · 설정에서 확인"
        );
    }

    #[test]
    fn autostart는_최초_성공_후_marker로_사용자_선택을_존중한다() {
        let dir = tempfile::tempdir().unwrap();
        let enabled = Cell::new(0);

        assert!(initialize_autostart_once(
            dir.path(),
            || {
                enabled.set(enabled.get() + 1);
                Ok(())
            },
            || Ok(())
        )
        .unwrap());
        assert!(autostart_marker(dir.path()).is_file());
        assert!(!initialize_autostart_once(
            dir.path(),
            || panic!("marker 이후에는 다시 enable하면 안 됨"),
            || Ok(())
        )
        .unwrap());
        assert_eq!(enabled.get(), 1);
    }

    #[test]
    fn autostart_enable_실패는_marker를_남기지_않아_재시도할_수_있다() {
        let dir = tempfile::tempdir().unwrap();

        let error = initialize_autostart_once(
            dir.path(),
            || Err("enable 실패".into()),
            || panic!("enable 실패 전에는 rollback할 필요 없음"),
        )
        .unwrap_err();

        assert_eq!(error, "enable 실패");
        assert!(!autostart_marker(dir.path()).exists());
    }

    #[test]
    fn autostart_marker가_directory면_초기화_호출_전에_거부한다() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(autostart_marker(dir.path())).unwrap();
        let enabled = Cell::new(0);
        let disabled = Cell::new(0);

        let error = initialize_autostart_once(
            dir.path(),
            || {
                enabled.set(enabled.get() + 1);
                Ok(())
            },
            || {
                disabled.set(disabled.get() + 1);
                Ok(())
            },
        )
        .unwrap_err();

        assert!(error.contains("marker가 일반 파일이 아님"));
        assert_eq!((enabled.get(), disabled.get()), (0, 0));
    }

    #[test]
    fn autostart_enable_후_marker_경합_오류는_enable을_rollback한다() {
        let dir = tempfile::tempdir().unwrap();
        let marker = autostart_marker(dir.path());
        let disabled = Cell::new(0);

        let error = initialize_autostart_once(
            dir.path(),
            || {
                std::fs::create_dir(&marker).unwrap();
                Ok(())
            },
            || {
                disabled.set(disabled.get() + 1);
                Ok(())
            },
        )
        .unwrap_err();

        assert!(error.contains("marker 기록 실패 후 자동시작 롤백"));
        assert_eq!(disabled.get(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn autostart_marker가_regular_or_dangling_symlink면_초기화_호출_전에_거부한다() {
        use std::os::unix::fs::symlink;

        for target_exists in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            let target = dir.path().join("marker-target");
            if target_exists {
                std::fs::write(&target, b"done").unwrap();
            }
            symlink(&target, autostart_marker(dir.path())).unwrap();
            let enabled = Cell::new(0);
            let disabled = Cell::new(0);

            let error = initialize_autostart_once(
                dir.path(),
                || {
                    enabled.set(enabled.get() + 1);
                    Ok(())
                },
                || {
                    disabled.set(disabled.get() + 1);
                    Ok(())
                },
            )
            .unwrap_err();

            assert!(error.contains("marker가 일반 파일이 아님"));
            assert_eq!((enabled.get(), disabled.get()), (0, 0));
        }
    }

    #[test]
    fn import는_완전한_staging만_publish하고_재시작에_기존_데이터를_backup한다() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let destination = root.path().join("destination");
        std::fs::create_dir_all(source.join("schemas")).unwrap();
        std::fs::write(
            source.join("schemas/new.yaml"),
            "type: 새타입\ncategory: 새분류\nfields:\n  이름: { kind: text }\n",
        )
        .unwrap();
        std::fs::write(
            source.join("categories.yaml"),
            "categories:\n  - { name: 새분류 }\n",
        )
        .unwrap();
        std::fs::create_dir_all(destination.join("schemas")).unwrap();
        std::fs::write(destination.join("schemas/old.yaml"), "old").unwrap();
        std::fs::write(destination.join("categories.yaml"), "old categories").unwrap();

        let runtime = tokio::runtime::Runtime::new().unwrap();
        let _destination_store = runtime.block_on(open_test_lifeops_db(&destination));
        runtime
            .block_on(stage_import(&source, &destination))
            .unwrap();

        assert_eq!(
            std::fs::read_to_string(destination.join("schemas/old.yaml")).unwrap(),
            "old"
        );
        assert!(destination
            .join(PENDING_IMPORT_DIR)
            .join(IMPORT_READY_MARKER)
            .is_file());
        assert!(runtime
            .block_on(apply_pending_import(&destination))
            .unwrap());
        assert_eq!(
            std::fs::read_to_string(destination.join("schemas/new.yaml")).unwrap(),
            "type: 새타입\ncategory: 새분류\nfields:\n  이름: { kind: text }\n"
        );
        assert_eq!(
            std::fs::read_to_string(destination.join("categories.yaml")).unwrap(),
            "categories:\n  - { name: 새분류 }\n"
        );
        let backup = std::fs::read_dir(destination.join(IMPORT_BACKUPS_DIR))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert_eq!(
            std::fs::read_to_string(backup.join("schemas/old.yaml")).unwrap(),
            "old"
        );
        assert_eq!(
            std::fs::read_to_string(backup.join("categories.yaml")).unwrap(),
            "old categories"
        );
    }

    #[test]
    fn import는_self와_ancestor_overlap을_거부한다() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("destination");
        std::fs::create_dir_all(&destination).unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();

        let self_error = runtime
            .block_on(stage_import(&destination, &destination))
            .unwrap_err();
        let ancestor_error = runtime
            .block_on(stage_import(root.path(), &destination))
            .unwrap_err();

        assert!(self_error.contains("겹칩니다"));
        assert!(ancestor_error.contains("겹칩니다"));
        assert!(!destination.join(PENDING_IMPORT_DIR).exists());
    }

    #[cfg(unix)]
    #[test]
    fn import는_root와_nested_symlink를_거부하고_partial_staging을_정리한다() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let destination = root.path().join("destination");
        std::fs::create_dir_all(source.join("schemas")).unwrap();
        std::fs::write(root.path().join("outside"), "secret").unwrap();
        symlink(root.path().join("outside"), source.join("schemas/escape")).unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();

        let nested_error = runtime
            .block_on(stage_import(&source, &destination))
            .unwrap_err();
        assert!(nested_error.contains("symlink"));
        assert!(!destination.join(PENDING_IMPORT_DIR).exists());

        let source_link = root.path().join("source-link");
        symlink(&source, &source_link).unwrap();
        let root_error = runtime
            .block_on(stage_import(&source_link, &destination))
            .unwrap_err();
        assert!(root_error.contains("symlink"));
    }

    #[test]
    fn import는_열린_source_sqlite의_consistent_snapshot을_만든다() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let destination = root.path().join("destination");
        let runtime = tokio::runtime::Runtime::new().unwrap();
        write_test_config(&destination);

        runtime.block_on(async {
            let open_source = open_test_lifeops_db(&source).await;
            let schemas =
                lifeops_core::schema::SchemaSet::load_dir(&destination.join("schemas")).unwrap();
            let mut data = serde_json::Map::new();
            data.insert("이름".into(), serde_json::json!("safe"));
            open_source.create(&schemas, "항목", data).await.unwrap();
            std::fs::write(source.join("data/lifeops.db-wal"), b"stale").unwrap();
            std::fs::write(source.join("data/lifeops.db-shm"), b"stale").unwrap();

            stage_import(&source, &destination).await.unwrap();
            let staged = destination.join(PENDING_IMPORT_DIR).join("data/lifeops.db");
            let options = SqliteConnectOptions::new().filename(staged).read_only(true);
            let mut snapshot = sqlx::SqliteConnection::connect_with(&options)
                .await
                .unwrap();
            let value: (String,) = sqlx::query_as("SELECT type FROM entities")
                .fetch_one(&mut snapshot)
                .await
                .unwrap();
            assert_eq!(value.0, "항목");
            assert!(!destination
                .join(PENDING_IMPORT_DIR)
                .join("data/lifeops.db-wal")
                .exists());
            assert!(!destination
                .join(PENDING_IMPORT_DIR)
                .join("data/lifeops.db-shm")
                .exists());
        });
    }

    #[test]
    fn import는_arbitrary_sqlite와_최종_page_source불일치를_거부한다() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("destination");
        write_test_config(&destination);
        let runtime = tokio::runtime::Runtime::new().unwrap();

        let arbitrary = root.path().join("arbitrary");
        std::fs::create_dir_all(arbitrary.join("data")).unwrap();
        runtime.block_on(async {
            let options = SqliteConnectOptions::new()
                .filename(arbitrary.join("data/lifeops.db"))
                .create_if_missing(true);
            let mut connection = sqlx::SqliteConnection::connect_with(&options)
                .await
                .unwrap();
            sqlx::query("CREATE TABLE sample(value TEXT)")
                .execute(&mut connection)
                .await
                .unwrap();
        });
        let error = runtime
            .block_on(stage_import(&arbitrary, &destination))
            .unwrap_err();
        assert!(error.contains("entities/refs"));
        assert!(!destination.join(PENDING_IMPORT_DIR).exists());

        let _store = runtime.block_on(open_test_lifeops_db(&destination));
        std::fs::create_dir_all(destination.join("views")).unwrap();
        std::fs::write(
            destination.join("views/home.yaml"),
            "page: 홈\nblocks:\n  - { view: 항목, source: 항목 }\n",
        )
        .unwrap();
        let source = root.path().join("source");
        std::fs::create_dir_all(source.join("schemas")).unwrap();
        std::fs::write(
            source.join("schemas/other.yaml"),
            "type: 다른항목\nfields:\n  이름: { kind: text }\n",
        )
        .unwrap();
        let error = runtime
            .block_on(stage_import(&source, &destination))
            .unwrap_err();
        assert!(error.contains("block source"));
        assert!(!destination.join(PENDING_IMPORT_DIR).exists());

        let invalid_page = root.path().join("invalid-page");
        std::fs::create_dir_all(invalid_page.join("views")).unwrap();
        std::fs::write(
            invalid_page.join("views/home.yaml"),
            "page: 홈\nblocks:\n  - view: 잘못된정렬\n    source: 항목\n    sort: 없는필드\n",
        )
        .unwrap();
        let error = runtime
            .block_on(stage_import(&invalid_page, &destination))
            .unwrap_err();
        assert!(error.contains("실행 검증 실패"));
        assert!(!destination.join(PENDING_IMPORT_DIR).exists());
    }

    #[test]
    fn import는_store필수_timestamp_columns가_없는_db를_거부한다() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("destination");
        write_test_config(&destination);
        let source = root.path().join("minimal-db");
        std::fs::create_dir_all(source.join("data")).unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let options = SqliteConnectOptions::new()
                .filename(source.join("data/lifeops.db"))
                .create_if_missing(true);
            let mut connection = sqlx::SqliteConnection::connect_with(&options)
                .await
                .unwrap();
            sqlx::raw_sql(
                "CREATE TABLE entities(id TEXT, type TEXT, data TEXT);\
                 CREATE TABLE refs(from_id TEXT, to_id TEXT, field_name TEXT);",
            )
            .execute(&mut connection)
            .await
            .unwrap();
        });

        let error = runtime
            .block_on(stage_import(&source, &destination))
            .unwrap_err();
        assert!(error.contains("필수 column 누락"));
        assert!(error.contains("created_at"));
    }

    #[test]
    fn import는_잘못된_entity_data와_dangling_refs를_거부한다() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("destination");
        write_test_config(&destination);
        let runtime = tokio::runtime::Runtime::new().unwrap();

        let bad_entity = root.path().join("bad-entity");
        runtime.block_on(async {
            let store = open_test_lifeops_db(&bad_entity).await;
            drop(store);
            let options = SqliteConnectOptions::new()
                .filename(bad_entity.join("data/lifeops.db"))
                .create_if_missing(false);
            let mut connection = sqlx::SqliteConnection::connect_with(&options).await.unwrap();
            sqlx::query("INSERT INTO entities(id,type,data,created_at,updated_at) VALUES('bad','항목','[]','now','now')")
                .execute(&mut connection)
                .await
                .unwrap();
        });
        let error = runtime
            .block_on(stage_import(&bad_entity, &destination))
            .unwrap_err();
        assert!(error.contains("JSON object"));

        let dangling = root.path().join("dangling");
        runtime.block_on(async {
            let store = open_test_lifeops_db(&dangling).await;
            let schemas =
                lifeops_core::schema::SchemaSet::load_dir(&destination.join("schemas")).unwrap();
            let mut data = serde_json::Map::new();
            data.insert("이름".into(), serde_json::json!("valid"));
            let entity = store.create(&schemas, "항목", data).await.unwrap();
            drop(store);
            let options = SqliteConnectOptions::new()
                .filename(dangling.join("data/lifeops.db"))
                .create_if_missing(false);
            let mut connection = sqlx::SqliteConnection::connect_with(&options)
                .await
                .unwrap();
            sqlx::query("INSERT INTO refs(from_id,to_id,field_name) VALUES(?, 'missing', 'ref')")
                .bind(entity.id)
                .execute(&mut connection)
                .await
                .unwrap();
        });
        let error = runtime
            .block_on(stage_import(&dangling, &destination))
            .unwrap_err();
        assert!(error.contains("dangling refs"));
    }

    #[test]
    fn import는_잘못된_top_level_type과_yaml을_publish하지_않는다() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("destination");
        let runtime = tokio::runtime::Runtime::new().unwrap();

        let wrong_type = root.path().join("wrong-type");
        std::fs::create_dir_all(&wrong_type).unwrap();
        std::fs::write(wrong_type.join("schemas"), "not a directory").unwrap();
        let error = runtime
            .block_on(stage_import(&wrong_type, &destination))
            .unwrap_err();
        assert!(error.contains("파일 타입"));
        assert!(!destination.join(PENDING_IMPORT_DIR).exists());

        let invalid_yaml = root.path().join("invalid-yaml");
        std::fs::create_dir_all(invalid_yaml.join("schemas")).unwrap();
        std::fs::write(invalid_yaml.join("schemas/broken.yaml"), "fields: [").unwrap();
        let error = runtime
            .block_on(stage_import(&invalid_yaml, &destination))
            .unwrap_err();
        assert!(error.contains("schema 검증 실패"));
        assert!(!destination.join(PENDING_IMPORT_DIR).exists());
        assert!(std::fs::read_dir(&destination).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(IMPORT_STAGING_PREFIX)));
    }

    #[test]
    fn incomplete_pending은_기존_destination을_변경하지_않는다() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path();
        std::fs::create_dir_all(destination.join(PENDING_IMPORT_DIR).join("schemas")).unwrap();
        std::fs::write(
            destination
                .join(PENDING_IMPORT_DIR)
                .join("schemas/new.yaml"),
            "partial",
        )
        .unwrap();
        std::fs::write(destination.join("categories.yaml"), "original").unwrap();

        let error = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(apply_pending_import(destination))
            .unwrap_err();

        assert!(error.contains("완료되지 않은"));
        assert_eq!(
            std::fs::read_to_string(destination.join("categories.yaml")).unwrap(),
            "original"
        );
    }

    #[test]
    fn committed_cleanup_residue는_다음_시작을_차단하지_않는다() {
        let root = tempfile::tempdir().unwrap();
        let pending = root.path().join(PENDING_IMPORT_DIR);
        std::fs::create_dir(&pending).unwrap();
        std::fs::write(pending.join(IMPORT_READY_MARKER), b"ready\n").unwrap();
        std::fs::write(root.path().join("categories.yaml"), "already promoted").unwrap();

        let runtime = tokio::runtime::Runtime::new().unwrap();
        assert!(runtime.block_on(apply_pending_import(root.path())).unwrap());
        assert!(!pending.exists());
        assert_eq!(
            std::fs::read_to_string(root.path().join("categories.yaml")).unwrap(),
            "already promoted"
        );

        std::fs::create_dir(&pending).unwrap();
        assert!(runtime.block_on(apply_pending_import(root.path())).unwrap());
        assert!(!pending.exists());
    }

    #[test]
    fn manifest는_backup완료_crash상태에서_payload를_재개한다() {
        let root = tempfile::tempdir().unwrap();
        let pending = root.path().join(PENDING_IMPORT_DIR);
        let backup_id = "resume-backup";
        let backup = root.path().join(IMPORT_BACKUPS_DIR).join(backup_id);
        std::fs::create_dir_all(&pending).unwrap();
        std::fs::create_dir_all(&backup).unwrap();
        std::fs::write(pending.join(IMPORT_READY_MARKER), b"ready\n").unwrap();
        std::fs::write(
            pending.join("categories.yaml"),
            "categories:\n  - { name: 새분류 }\n",
        )
        .unwrap();
        std::fs::write(backup.join("categories.yaml"), "old categories").unwrap();
        let manifest = ImportManifest {
            version: 1,
            backup_id: backup_id.into(),
            items: vec![ImportManifestItem {
                name: "categories.yaml".into(),
                had_target: true,
                phase: ImportPhase::Staged,
            }],
        };
        write_manifest(&pending.join(IMPORT_MANIFEST), &manifest).unwrap();

        let runtime = tokio::runtime::Runtime::new().unwrap();
        assert!(runtime.block_on(apply_pending_import(root.path())).unwrap());
        assert_eq!(
            std::fs::read_to_string(root.path().join("categories.yaml")).unwrap(),
            "categories:\n  - { name: 새분류 }\n"
        );
        assert_eq!(
            std::fs::read_to_string(backup.join("categories.yaml")).unwrap(),
            "old categories"
        );
        assert!(!pending.exists());
    }

    #[cfg(unix)]
    #[test]
    fn apply는_backup_root와_leaf_symlink를_거부한다() {
        use std::os::unix::fs::symlink;

        for leaf in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let pending = root.path().join(PENDING_IMPORT_DIR);
            std::fs::create_dir(&pending).unwrap();
            std::fs::write(pending.join(IMPORT_READY_MARKER), b"ready\n").unwrap();
            std::fs::write(pending.join("categories.yaml"), "categories: []\n").unwrap();
            let manifest = ImportManifest {
                version: 1,
                backup_id: "unsafe".into(),
                items: vec![ImportManifestItem {
                    name: "categories.yaml".into(),
                    had_target: false,
                    phase: ImportPhase::Staged,
                }],
            };
            write_manifest(&pending.join(IMPORT_MANIFEST), &manifest).unwrap();
            let outside = root.path().join("outside");
            std::fs::create_dir(&outside).unwrap();
            if leaf {
                std::fs::create_dir(root.path().join(IMPORT_BACKUPS_DIR)).unwrap();
                symlink(
                    &outside,
                    root.path().join(IMPORT_BACKUPS_DIR).join("unsafe"),
                )
                .unwrap();
            } else {
                symlink(&outside, root.path().join(IMPORT_BACKUPS_DIR)).unwrap();
            }

            let error = tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(apply_pending_import(root.path()))
                .unwrap_err();
            assert!(error.contains("no-follow directory openat"));
            assert!(pending.join("categories.yaml").is_file());
        }
    }
}
