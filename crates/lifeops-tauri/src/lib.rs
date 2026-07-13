use lifeops_server::{default_data_dir, resolve_paths, serve, RunConfig};
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use tauri::async_runtime::spawn;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, Url, WebviewWindow, WindowEvent, Wry};
use tauri_plugin_autostart::ManagerExt;

const AUTOSTART_INIT_MARKER: &str = ".autostart-initialized";

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
                    if let Err(error) = addr_i.set_text(tray_address_label(addr.port())) {
                        tracing::error!(%error, "트레이 LAN 주소 갱신 실패");
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

fn tray_address_label(port: u16) -> String {
    tray_address_label_from(port, lifeops_server::routes::system::lan_addresses(port))
}

fn tray_address_label_from(port: u16, addresses: Vec<String>) -> String {
    addresses
        .into_iter()
        .next()
        .map(|url| format!("LAN 주소: {url}"))
        .unwrap_or_else(|| format!("LAN 주소: 포트 {port} · 설정에서 확인"))
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
    fn tray_lan_label은_실제_주소를_우선하고_없으면_정확한_port를_안내한다() {
        assert_eq!(
            tray_address_label_from(3012, vec!["http://192.168.0.7:3012".into()]),
            "LAN 주소: http://192.168.0.7:3012"
        );
        assert_eq!(
            tray_address_label_from(3012, vec![]),
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
}
