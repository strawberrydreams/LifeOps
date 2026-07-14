use crate::routes::{entities, misc, pages, schemas, search, system};
use crate::state::AppState;
use crate::static_files::static_handler;
use axum::middleware::from_fn_with_state;
use axum::routing::{delete, get, post, put};
use axum::Router;
use tower_http::limit::RequestBodyLimitLayer;

const MCP_REQUEST_BODY_LIMIT: usize = 2 * 1024 * 1024;

pub fn build_app(state: AppState) -> Router {
    build_app_with_mcp_auth(state, crate::mcp::auth::AuthPolicy::from_env())
}

pub(crate) fn build_app_with_mcp_auth(
    state: AppState,
    auth_policy: crate::mcp::auth::AuthPolicy,
) -> Router {
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
        .route(
            "/api/system/config",
            get(system::config_get).merge(put(system::config_put)),
        )
        .route("/api/system/backup", post(system::backup_create))
        .route("/api/system/backups", get(system::backups_list))
        .route("/api/reload", post(misc::reload));

    let mcp = Router::new()
        .nest_service("/mcp", crate::mcp::service(state.clone()))
        .layer(RequestBodyLimitLayer::new(MCP_REQUEST_BODY_LIMIT))
        .layer(from_fn_with_state(auth_policy, crate::mcp::guard));

    Router::new()
        .merge(api)
        .merge(mcp)
        .fallback(static_handler)
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::test_state;
    use axum::body::{to_bytes, Body};
    use axum::extract::ConnectInfo;
    use axum::http::{header::CONTENT_LENGTH, header::ORIGIN, HeaderValue, Request, StatusCode};
    use std::net::SocketAddr;
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

    fn mcp_initialize_request(peer: &str, host: &str, bearer: Option<&str>) -> Request<Body> {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": { "name": "lifeops-test", "version": "1.0" }
            }
        });
        let mut builder = Request::builder()
            .method("POST")
            .uri("/mcp")
            .header("host", host)
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream");
        if let Some(token) = bearer {
            builder = builder.header("authorization", format!("Bearer {token}"));
        }
        let mut request = builder.body(Body::from(body.to_string())).unwrap();
        request
            .extensions_mut()
            .insert(ConnectInfo(peer.parse::<SocketAddr>().unwrap()));
        request
    }

    fn with_origin(mut request: Request<Body>, origin: &str) -> Request<Body> {
        request
            .headers_mut()
            .insert(ORIGIN, HeaderValue::from_str(origin).unwrap());
        request
    }

    #[tokio::test]
    async fn mcp_토큰없음_loopback의_유효한_initialize는_200이다() {
        let (state, _dir) = test_state().await;
        let app = build_app_with_mcp_auth(state, crate::mcp::auth::AuthPolicy::from_token(None));

        let response = app
            .oneshot(mcp_initialize_request(
                "127.0.0.1:5555",
                "127.0.0.1:3000",
                None,
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().contains_key("mcp-session-id"));
    }

    #[tokio::test]
    async fn mcp_토큰없음_비loopback은_403이다() {
        let (state, _dir) = test_state().await;
        let app = build_app_with_mcp_auth(state, crate::mcp::auth::AuthPolicy::from_token(None));

        let response = app
            .oneshot(mcp_initialize_request(
                "192.168.10.9:5555",
                "192.168.10.9:3000",
                None,
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn mcp_토큰_불일치는_loopback에서도_401이다() {
        let (state, _dir) = test_state().await;
        let app = build_app_with_mcp_auth(
            state,
            crate::mcp::auth::AuthPolicy::from_token(Some("s3cr3t".into())),
        );

        let response = app
            .oneshot(mcp_initialize_request(
                "127.0.0.1:5555",
                "127.0.0.1:3000",
                Some("wrong"),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(response.headers()["www-authenticate"], "Bearer");
    }

    #[tokio::test]
    async fn mcp_토큰_일치_및_허용된_lan_host는_initialize에_도달한다() {
        let (mut state, _dir) = test_state().await;
        state.bound_addr = "192.168.10.4:3000".parse().unwrap();
        let app = build_app_with_mcp_auth(
            state,
            crate::mcp::auth::AuthPolicy::from_token(Some("s3cr3t".into())),
        );

        let response = app
            .oneshot(mcp_initialize_request(
                "192.168.10.9:5555",
                "192.168.10.4:3000",
                Some("s3cr3t"),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().contains_key("mcp-session-id"));
    }

    #[tokio::test]
    async fn mcp_적대적_host는_올바른_토큰에도_403이다() {
        let (state, _dir) = test_state().await;
        let app = build_app_with_mcp_auth(
            state,
            crate::mcp::auth::AuthPolicy::from_token(Some("s3cr3t".into())),
        );

        let response = app
            .oneshot(mcp_initialize_request(
                "127.0.0.1:5555",
                "evil.example:3000",
                Some("s3cr3t"),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn mcp_적대적_origin은_올바른_host와_토큰에도_403이다() {
        let (mut state, _dir) = test_state().await;
        state.bound_addr = "127.0.0.1:3000".parse().unwrap();
        let app = build_app_with_mcp_auth(
            state,
            crate::mcp::auth::AuthPolicy::from_token(Some("s3cr3t".into())),
        );
        let request = with_origin(
            mcp_initialize_request("127.0.0.1:5555", "127.0.0.1:3000", Some("s3cr3t")),
            "https://evil.example",
        );

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn mcp_같은_origin은_initialize에_도달한다() {
        let (mut state, _dir) = test_state().await;
        state.bound_addr = "127.0.0.1:3000".parse().unwrap();
        let app = build_app_with_mcp_auth(
            state,
            crate::mcp::auth::AuthPolicy::from_token(Some("s3cr3t".into())),
        );
        let request = with_origin(
            mcp_initialize_request("127.0.0.1:5555", "127.0.0.1:3000", Some("s3cr3t")),
            "http://127.0.0.1:3000",
        );

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().contains_key("mcp-session-id"));
    }

    #[tokio::test]
    async fn mcp_content_length가_본문제한을_넘으면_413이다() {
        let (state, _dir) = test_state().await;
        let app = build_app_with_mcp_auth(
            state,
            crate::mcp::auth::AuthPolicy::from_token(Some("s3cr3t".into())),
        );
        let mut request =
            mcp_initialize_request("127.0.0.1:5555", "127.0.0.1:3000", Some("s3cr3t"));
        let oversized = MCP_REQUEST_BODY_LIMIT + 1;
        request.headers_mut().insert(
            CONTENT_LENGTH,
            HeaderValue::from_str(&oversized.to_string()).unwrap(),
        );
        *request.body_mut() = Body::from(vec![b'x'; oversized]);

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }
}
