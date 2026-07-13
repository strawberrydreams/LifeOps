use lifeops_server::{default_data_dir, resolve_paths, serve, RunConfig};
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use std::path::Path;
use tauri::async_runtime::spawn;
use tauri::{Manager, Url, WebviewWindow};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = tracing_subscriber::fmt().try_init();
    let data_dir = default_data_dir();

    let mut builder = tauri::Builder::default();

    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_focus();
            }
        }));
    }

    builder
        .setup(move |app| {
            let handle = app.handle().clone();
            let data_dir = data_dir.clone();
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
                if let Ok((_addr, future)) = result {
                    future.await;
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Tauri 실행 실패");
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
}
