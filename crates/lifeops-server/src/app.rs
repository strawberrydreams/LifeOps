use crate::state::AppState;
use axum::routing::get;
use axum::Router;

pub fn build_app(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(|| async { "ok" }))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::test_state;
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    #[tokio::test]
    async fn health_엔드포인트() {
        let (state, _dir) = test_state().await;
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
        let body = to_bytes(res.into_body(), 1 << 20).await.unwrap();
        assert_eq!(&body[..], b"ok");
    }
}
