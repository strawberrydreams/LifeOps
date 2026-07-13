use axum::http::{header, Uri};
use axum::response::{Html, IntoResponse, Response};

#[cfg(feature = "embed-spa")]
#[derive(rust_embed::RustEmbed)]
#[folder = "../../frontend/dist"]
struct SpaAssets;

/// 정적 에셋 제공, 매칭 없으면 SPA index.html(클라이언트 라우팅) fallback.
#[cfg(feature = "embed-spa")]
pub async fn static_handler(uri: Uri) -> Response {
    let rel = uri.path().trim_start_matches('/');
    if !rel.is_empty() {
        if let Some(file) = SpaAssets::get(rel) {
            let mime = mime_guess::from_path(rel).first_or_octet_stream();
            return (
                [(header::CONTENT_TYPE, mime.as_ref())],
                file.data.into_owned(),
            )
                .into_response();
        }
    }
    match SpaAssets::get("index.html") {
        Some(index) => Html(index.data.into_owned()).into_response(),
        None => (
            axum::http::StatusCode::NOT_FOUND,
            "index.html 없음: frontend 빌드 필요",
        )
            .into_response(),
    }
}

/// 개발 모드: frontend/dist 를 디스크에서 제공.
#[cfg(not(feature = "embed-spa"))]
pub async fn static_handler(uri: Uri) -> Response {
    let rel = uri.path().trim_start_matches('/');
    let base = std::path::Path::new("frontend/dist");
    if !rel.is_empty() {
        if let Some((candidate, bytes)) = read_disk_asset(base, rel).await {
            let mime = mime_guess::from_path(candidate).first_or_octet_stream();
            return ([(header::CONTENT_TYPE, mime.as_ref())], bytes).into_response();
        }
    }
    match tokio::fs::read_to_string(base.join("index.html")).await {
        Ok(html) => Html(html).into_response(),
        Err(_) => Html("<!doctype html><meta charset=utf-8><h1>LifeOps</h1><p>프론트엔드 빌드가 아직 없습니다. API는 <code>/api/*</code>에서 동작합니다.</p>").into_response(),
    }
}

#[cfg(not(feature = "embed-spa"))]
async fn read_disk_asset(
    base: &std::path::Path,
    relative: &str,
) -> Option<(std::path::PathBuf, Vec<u8>)> {
    use std::path::Component;

    let safe = std::path::Path::new(relative)
        .components()
        .all(|component| matches!(component, Component::Normal(_)));
    if !safe {
        return None;
    }

    let base = tokio::fs::canonicalize(base).await.ok()?;
    let candidate = base.join(relative);
    let canonical_candidate = tokio::fs::canonicalize(&candidate).await.ok()?;
    if !canonical_candidate.starts_with(&base) {
        return None;
    }
    let bytes = tokio::fs::read(canonical_candidate).await.ok()?;
    Some((candidate, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "embed-spa")]
    use axum::body::to_bytes;
    use axum::http::{StatusCode, Uri};

    #[tokio::test]
    async fn 미존재_경로는_spa_fallback_200() {
        let res = static_handler("/무엇이든".parse::<Uri>().unwrap()).await;
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[cfg(feature = "embed-spa")]
    #[tokio::test]
    async fn 루트는_임베드된_index를_반환한다() {
        let expected = SpaAssets::get("index.html").expect("frontend build의 index.html");
        let res = static_handler("/".parse::<Uri>().unwrap()).await;

        assert_eq!(res.status(), StatusCode::OK);
        let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        assert_eq!(body.as_ref(), expected.data.as_ref());
    }

    #[cfg(feature = "embed-spa")]
    #[tokio::test]
    async fn asset은_임베드된_바이트와_content_type을_반환한다() {
        let path = SpaAssets::iter()
            .find(|path| path.starts_with("assets/"))
            .expect("frontend build의 assets 파일");
        let expected = SpaAssets::get(path.as_ref()).expect("열거된 asset");
        let expected_mime = mime_guess::from_path(path.as_ref()).first_or_octet_stream();
        let uri = format!("/{path}").parse::<Uri>().unwrap();
        let res = static_handler(uri).await;

        assert_eq!(res.status(), StatusCode::OK);
        let content_type = res
            .headers()
            .get(header::CONTENT_TYPE)
            .expect("asset content type")
            .to_str()
            .unwrap();
        assert!(!content_type.is_empty());
        assert_eq!(content_type, expected_mime.as_ref());
        let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        assert_eq!(body.as_ref(), expected.data.as_ref());
    }

    #[cfg(all(not(feature = "embed-spa"), unix))]
    #[tokio::test]
    async fn disk_asset은_symlink로_정적_루트_밖을_읽지_않는다() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("dist");
        std::fs::create_dir(&base).unwrap();
        let secret = dir.path().join("secret.txt");
        std::fs::write(&secret, b"secret").unwrap();
        symlink(&secret, base.join("escape.txt")).unwrap();

        assert!(read_disk_asset(&base, "escape.txt").await.is_none());
    }
}
