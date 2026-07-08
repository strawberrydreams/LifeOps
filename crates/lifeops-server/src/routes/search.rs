use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
pub struct SearchParams {
    #[serde(default)]
    pub q: String,
    pub limit: Option<usize>,
}

/// GET /api/search?q=<query>&limit=<n=50>
pub async fn search(
    State(st): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Result<Json<Value>, ApiError> {
    let schemas = st.schemas.read().await;
    let limit = params.limit.unwrap_or(50);
    let results = st.store.search(&schemas, &params.q, limit).await?;
    Ok(Json(json!(results)))
}

#[cfg(test)]
mod tests {
    use crate::app::build_app;
    use crate::state::test_state;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use serde_json::{json, Value};
    use tower::ServiceExt;

    async fn body_json(res: axum::response::Response) -> Value {
        let bytes = axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    fn post(uri: &str, body: Value) -> Request<Body> {
        Request::builder().method("POST").uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string())).unwrap()
    }

    #[tokio::test]
    async fn 검색이_여러_타입을_반환하고_href를_준다() {
        let (state, _dir) = test_state().await;
        let app = build_app(state);
        app.clone().oneshot(post("/api/entities", json!({ "type": "시계", "data": { "이름": "세이코 미쿠" } }))).await.unwrap();
        app.clone().oneshot(post("/api/entities", json!({ "type": "프로필", "data": { "이름": "세이코 팬" } }))).await.unwrap();

        // "%EC%84%B8%EC%9D%B4%EC%BD%94" = "세이코"
        let res = app.oneshot(Request::builder().uri("/api/search?q=%EC%84%B8%EC%9D%B4%EC%BD%94").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = body_json(res).await;
        assert!(body["total"].as_u64().unwrap() >= 2);
        let hrefs: Vec<&str> = body["results"].as_array().unwrap().iter().map(|h| h["href"].as_str().unwrap()).collect();
        assert!(hrefs.iter().any(|h| h.starts_with("/entity/")));
        assert!(hrefs.contains(&"/pages/프로필"));
        // match 오프셋 필드 존재
        assert!(body["results"][0]["match"].get("start").is_some());
    }

    #[tokio::test]
    async fn 빈_쿼리는_빈_결과() {
        let (state, _dir) = test_state().await;
        let app = build_app(state);
        let res = app.oneshot(Request::builder().uri("/api/search?q=").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = body_json(res).await;
        assert_eq!(body["total"], json!(0));
        assert_eq!(body["results"].as_array().unwrap().len(), 0);
    }
}
