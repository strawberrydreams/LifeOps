use crate::routes::{entities, misc, pages, schemas, search, system};
use crate::state::AppState;
use crate::static_files::static_handler;
use axum::routing::{delete, get, post, put};
use axum::Router;

pub fn build_app(state: AppState) -> Router {
    let api = Router::new()
        .route("/api/health", get(|| async { "ok" }))
        .route("/api/entities", get(entities::list).post(entities::create))
        .route(
            "/api/entities/{id}",
            get(entities::get_one)
                .patch(entities::update)
                .delete(entities::delete),
        )
        .route("/api/schemas", get(misc::schemas).post(schemas::create))
        .route(
            "/api/schemas/{type}",
            get(schemas::get_one)
                .merge(put(schemas::update))
                .merge(delete(schemas::delete)),
        )
        .route("/api/pages", get(pages::list).post(pages::create))
        .route("/api/pages/preview", post(pages::preview))
        .route(
            "/api/pages/{name}",
            get(misc::page)
                .merge(put(pages::update))
                .merge(delete(pages::delete)),
        )
        .route("/api/export", get(misc::export))
        .route("/api/search", get(search::search))
        .route("/api/system/info", get(system::info))
        .route("/api/reload", post(misc::reload));

    Router::new()
        .merge(api)
        .fallback(static_handler)
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
