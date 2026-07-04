use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use lifeops_core::schema::SchemaSet;
use lifeops_core::view::{run_page, PageSet};
use serde_json::{json, Map, Value};

/// GET /api/schemas → { types: { 타입명: ResolvedSchema }, categories: [CategoryDef] }
pub async fn schemas(State(st): State<AppState>) -> Json<Value> {
    let schemas = st.schemas.read().await;
    let categories = st.categories.read().await;
    let mut types = Map::new();
    for name in schemas.names() {
        if let Some(s) = schemas.get(name) {
            types.insert(name.to_string(), json!(s));
        }
    }
    Json(json!({ "types": types, "categories": *categories }))
}

/// GET /api/pages/:name → PageResult
pub async fn page(
    State(st): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let pages = st.pages.read().await;
    let Some(def) = pages.get(&name) else {
        return Err(ApiError(
            StatusCode::NOT_FOUND,
            json!({ "error": { "code": "not_found", "message": format!("페이지 없음: {name}") } }),
        ));
    };
    let schemas = st.schemas.read().await;
    let result = run_page(&st.store, &schemas, def).await?;
    Ok(Json(json!(result)))
}

/// GET /api/export → { 타입명: [엔티티...] }
pub async fn export(State(st): State<AppState>) -> Result<Json<Value>, ApiError> {
    let schemas = st.schemas.read().await;
    let mut out = Map::new();
    for name in schemas.names() {
        let items = st.store.list(&[name.to_string()]).await?;
        out.insert(name.to_string(), json!(items));
    }
    Ok(Json(Value::Object(out)))
}

/// POST /api/reload → 성공 시 스키마·페이지 교체, 실패 시 기존 유지 + 에러
pub async fn reload(State(st): State<AppState>) -> Result<Json<Value>, ApiError> {
    // 새로 파싱 (실패하면 여기서 반환되어 기존 상태 유지)
    let new_schemas = SchemaSet::load_dir(&st.schemas_dir).map_err(|e| ApiError(
        StatusCode::BAD_REQUEST,
        json!({ "error": { "code": "reload_schema", "message": e.to_string() } }),
    ))?;
    let new_pages = PageSet::load_dir(&st.views_dir)?;
    let new_categories =
        lifeops_core::schema::load_categories(&st.categories_path).map_err(|e| {
            ApiError(
                StatusCode::BAD_REQUEST,
                json!({ "error": { "code": "reload_categories", "message": e.to_string() } }),
            )
        })?;
    // 세 락을 한 스코프에서 잡고 함께 교체해, 어떤 요청도
    // "새 스키마 + 옛 페이지" 같은 중간 상태를 관찰할 수 없게 한다.
    let mut s = st.schemas.write().await;
    let mut p = st.pages.write().await;
    let mut c = st.categories.write().await;
    *s = new_schemas;
    *p = new_pages;
    *c = new_categories;
    drop(c);
    drop(p);
    drop(s);
    Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use crate::app::build_app;
    use crate::state::test_state;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use serde_json::Value;
    use tower::ServiceExt;

    async fn body_json(res: axum::response::Response) -> Value {
        let bytes = axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn schemas_반환_types와_categories() {
        let (state, _d) = test_state().await;
        let app = build_app(state);
        let res = app.oneshot(Request::builder().uri("/api/schemas").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = body_json(res).await;
        assert!(body["types"].get("물건").is_some());
        assert!(body["types"]["시계"]["fields"].get("가격").is_some()); // 상속 병합
        assert_eq!(body["types"]["물건"]["category"], "컬렉션");
        assert_eq!(body["categories"][0]["name"], "할일");
    }

    #[tokio::test]
    async fn reload는_categories도_갱신() {
        let (state, dir) = test_state().await;
        let cat_path = dir.path().join("categories.yaml");
        std::fs::write(&cat_path, "categories:\n  - { name: 새카테고리 }\n").unwrap();
        let app = build_app(state);
        app.clone().oneshot(Request::builder().method("POST").uri("/api/reload").body(Body::empty()).unwrap()).await.unwrap();
        let body = body_json(app.oneshot(Request::builder().uri("/api/schemas").body(Body::empty()).unwrap()).await.unwrap()).await;
        assert_eq!(body["categories"][0]["name"], "새카테고리");
    }

    #[tokio::test]
    async fn export_왕복() {
        let (state, _d) = test_state().await;
        let app = build_app(state);
        // 하나 생성
        app.clone().oneshot(Request::builder().method("POST").uri("/api/entities")
            .header("content-type","application/json")
            .body(Body::from(serde_json::json!({"type":"시계","data":{"이름":"미쿠"}}).to_string())).unwrap()).await.unwrap();
        let res = app.oneshot(Request::builder().uri("/api/export").body(Body::empty()).unwrap()).await.unwrap();
        let body = body_json(res).await;
        assert_eq!(body["시계"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn reload_성공() {
        let (state, _d) = test_state().await;
        let app = build_app(state);
        let res = app.oneshot(Request::builder().method("POST").uri("/api/reload").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn 없는_페이지는_404() {
        let (state, _d) = test_state().await;
        let app = build_app(state);
        let res = app.oneshot(Request::builder().uri("/api/pages/없는페이지").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }
}
