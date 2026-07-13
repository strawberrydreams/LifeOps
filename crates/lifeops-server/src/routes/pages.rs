use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use lifeops_core::view::{load_page_files, run_page, to_yaml, PageDef, PageSet};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static PAGE_TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// GET /api/pages → { pages: [PageDef...] } (로드 순서)
pub async fn list(State(st): State<AppState>) -> Json<Value> {
    let pages = st.pages.read().await;
    Json(json!({ "pages": pages.all() }))
}

/// POST /api/pages — 새 페이지 생성.
pub async fn create(
    State(st): State<AppState>,
    Json(def): Json<PageDef>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let name = def.page.trim().to_string();
    if name.is_empty() {
        return Err(bad_request("bad_page_name", "page 이름은 필수입니다"));
    }
    let filename = safe_page_filename(&name)?;
    let mut def = def;
    def.page = name.clone();
    persist_page(&st, &name, &filename, def, true).await?;
    Ok((StatusCode::CREATED, Json(json!({ "ok": true }))))
}

/// PUT /api/pages/:name — 페이지 수정(경로 이름을 정본으로).
pub async fn update(
    State(st): State<AppState>,
    Path(name): Path<String>,
    Json(def): Json<PageDef>,
) -> Result<Json<Value>, ApiError> {
    let filename = safe_page_filename(&name)?;
    let mut def = def;
    def.page = name.clone();
    persist_page(&st, &name, &filename, def, false).await?;
    Ok(Json(json!({ "ok": true })))
}

/// DELETE /api/pages/:name
pub async fn delete(
    State(st): State<AppState>,
    Path(name): Path<String>,
) -> Result<StatusCode, ApiError> {
    safe_page_filename(&name)?;
    delete_page(&st, &name).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/pages/preview — PageDef를 저장 없이 실행해 PageResult 반환.
pub async fn preview(
    State(st): State<AppState>,
    Json(def): Json<PageDef>,
) -> Result<Json<Value>, ApiError> {
    let schemas = st.schemas.read().await;
    let result = run_page(&st.store, &schemas, &def).await?;
    Ok(Json(json!(result)))
}

/// 후보 페이지 집합을 run_page로 검증한 뒤 통과 시에만 파일을 원자적으로 교체하고 메모리 PageSet을 갱신한다.
/// 락 순서: schemas(read) → pages(write) (reload과 동일, 데드락 회피).
async fn persist_page(
    st: &AppState,
    name: &str,
    filename: &str,
    def: PageDef,
    is_create: bool,
) -> Result<(), ApiError> {
    let schemas = st.schemas.read().await;
    let mut pages = st.pages.write().await;

    let mut candidates = load_page_files(&st.views_dir).map_err(ApiError::from)?;
    let existing_filename = candidates.get(name).map(|(_, file)| file.clone());

    if is_create {
        if pages.get(name).is_some()
            || existing_filename.is_some()
            || st.views_dir.join(filename).exists()
        {
            return Err(ApiError(
                StatusCode::CONFLICT,
                json!({ "error": { "code": "page_exists", "message": format!("이미 존재하는 페이지입니다: {name}") } }),
            ));
        }
    } else if pages.get(name).is_none() {
        return Err(ApiError(
            StatusCode::NOT_FOUND,
            json!({ "error": { "code": "unknown_page", "message": format!("알 수 없는 페이지 '{name}'") } }),
        ));
    }

    run_page(&st.store, &schemas, &def).await?;

    let target_file = existing_filename.unwrap_or_else(|| filename.to_string());
    let yaml = to_yaml(&def).map_err(|err| {
        tracing::error!("페이지 직렬화 오류: {err}");
        internal("page_serialize")
    })?;
    candidates.insert(name.to_string(), (def, target_file.clone()));
    let new_set = PageSet::from_files(candidates);

    let tmp_path = st.views_dir.join(unique_tmp_filename(&target_file));
    let final_path = st.views_dir.join(&target_file);
    std::fs::write(&tmp_path, yaml).map_err(|err| io_error("page_write", err))?;
    if let Err(err) = std::fs::rename(&tmp_path, &final_path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(io_error("page_write", err));
    }

    *pages = new_set;
    Ok(())
}

async fn delete_page(st: &AppState, name: &str) -> Result<(), ApiError> {
    let mut pages = st.pages.write().await;
    if pages.get(name).is_none() {
        return Err(ApiError(
            StatusCode::NOT_FOUND,
            json!({ "error": { "code": "unknown_page", "message": format!("알 수 없는 페이지 '{name}'") } }),
        ));
    }
    let mut candidates = load_page_files(&st.views_dir).map_err(ApiError::from)?;
    let Some((_, filename)) = candidates.get(name).cloned() else {
        tracing::error!("메모리에는 있지만 파일에는 없는 페이지: {name}");
        return Err(internal("page_load"));
    };
    candidates.shift_remove(name);
    let new_set = PageSet::from_files(candidates);
    std::fs::remove_file(st.views_dir.join(filename))
        .map_err(|err| io_error("page_delete", err))?;
    *pages = new_set;
    Ok(())
}

fn safe_page_filename(name: &str) -> Result<String, ApiError> {
    if name.trim().is_empty()
        || name == "new"
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
    {
        return Err(bad_request(
            "bad_page_name",
            "안전하지 않은 페이지 이름입니다",
        ));
    }
    Ok(format!("{name}.yaml"))
}

fn unique_tmp_filename(filename: &str) -> String {
    let seq = PAGE_TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{filename}.{}.{}.{}.tmp", std::process::id(), nanos, seq)
}

fn bad_request(code: &str, message: &str) -> ApiError {
    ApiError(
        StatusCode::BAD_REQUEST,
        json!({ "error": { "code": code, "message": message } }),
    )
}

fn internal(code: &str) -> ApiError {
    ApiError(
        StatusCode::INTERNAL_SERVER_ERROR,
        json!({ "error": { "code": code, "message": "내부 서버 오류" } }),
    )
}

fn io_error(code: &str, err: std::io::Error) -> ApiError {
    tracing::error!("페이지 파일 오류: {err}");
    internal(code)
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
        let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    fn post(uri: &str, body: Value) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    fn get(uri: &str) -> Request<Body> {
        Request::builder().uri(uri).body(Body::empty()).unwrap()
    }

    fn put(uri: &str, body: Value) -> Request<Body> {
        Request::builder()
            .method("PUT")
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    fn del(uri: &str) -> Request<Body> {
        Request::builder()
            .method("DELETE")
            .uri(uri)
            .body(Body::empty())
            .unwrap()
    }

    #[tokio::test]
    async fn 페이지_생성후_목록에_나온다() {
        let (state, _dir) = test_state().await;
        let vdir = state.views_dir.clone();
        let app = build_app(state);
        let res = app
            .clone()
            .oneshot(post(
                "/api/pages",
                json!({
                    "page": "대시보드",
                    "blocks": [ { "view": "할 일", "source": "할일", "filter": { "완료": false }, "layout": "checklist" } ]
                }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);
        assert!(vdir.join("대시보드.yaml").exists());

        let body = body_json(app.oneshot(get("/api/pages")).await.unwrap()).await;
        let pages = body["pages"].as_array().unwrap();
        assert!(pages.iter().any(|p| p["page"] == "대시보드"));
    }

    #[tokio::test]
    async fn 없는_source_페이지_생성은_400이고_파일을_남기지_않는다() {
        let (state, _dir) = test_state().await;
        let vdir = state.views_dir.clone();
        let app = build_app(state);
        let res = app
            .oneshot(post(
                "/api/pages",
                json!({ "page": "깨진페이지", "blocks": [ { "view": "v", "source": "유령타입" } ] }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        assert!(!vdir.join("깨진페이지.yaml").exists());
    }

    #[tokio::test]
    async fn 이미_있는_페이지_생성은_409() {
        let (state, _dir) = test_state().await;
        let app = build_app(state);
        let dup = app
            .oneshot(post(
                "/api/pages",
                json!({ "page": "프로필", "blocks": [] }),
            ))
            .await
            .unwrap();
        assert_eq!(dup.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn 예약된_new_페이지명은_400이고_파일을_남기지_않는다() {
        let (state, _dir) = test_state().await;
        let vdir = state.views_dir.clone();
        let app = build_app(state);
        let res = app
            .oneshot(post(
                "/api/pages",
                json!({ "page": "new", "blocks": [] }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        assert_eq!(body_json(res).await["error"]["code"], "bad_page_name");
        assert!(!vdir.join("new.yaml").exists());
    }

    #[tokio::test]
    async fn 페이지_수정이_반영된다() {
        let (state, _dir) = test_state().await;
        let app = build_app(state);
        app.clone()
            .oneshot(post(
                "/api/pages",
                json!({ "page": "대시보드", "blocks": [ { "view": "할 일", "source": "할일", "layout": "checklist" } ] }),
            ))
            .await
            .unwrap();

        let res = app
            .clone()
            .oneshot(put(
                "/api/pages/대시보드",
                json!({
                    "page": "본문의다른이름",
                    "blocks": [
                        { "view": "할 일", "source": "할일", "layout": "checklist" },
                        { "view": "지출", "source": "물건", "aggregate": { "합계": "sum(가격)" } }
                    ]
                }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let body = body_json(app.oneshot(get("/api/pages")).await.unwrap()).await;
        let page = body["pages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["page"] == "대시보드")
            .unwrap();
        assert_eq!(page["blocks"].as_array().unwrap().len(), 2);
        assert!(!body["pages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["page"] == "본문의다른이름"));
    }

    #[tokio::test]
    async fn 없는_페이지_수정은_404() {
        let (state, _dir) = test_state().await;
        let app = build_app(state);
        let res = app
            .oneshot(put(
                "/api/pages/유령",
                json!({ "page": "유령", "blocks": [] }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn 페이지_삭제후_목록에서_사라진다() {
        let (state, _dir) = test_state().await;
        let vdir = state.views_dir.clone();
        let app = build_app(state);
        app.clone()
            .oneshot(post("/api/pages", json!({ "page": "임시", "blocks": [] })))
            .await
            .unwrap();
        assert!(vdir.join("임시.yaml").exists());

        let res = app.clone().oneshot(del("/api/pages/임시")).await.unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        assert!(!vdir.join("임시.yaml").exists());

        let body = body_json(app.oneshot(get("/api/pages")).await.unwrap()).await;
        assert!(!body["pages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["page"] == "임시"));
    }

    #[tokio::test]
    async fn 없는_페이지_삭제는_404() {
        let (state, _dir) = test_state().await;
        let app = build_app(state);
        let res = app.oneshot(del("/api/pages/유령")).await.unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn preview는_저장없이_결과를_돌려준다() {
        let (state, _dir) = test_state().await;
        let vdir = state.views_dir.clone();
        let app = build_app(state);
        app.clone()
            .oneshot(post(
                "/api/entities",
                json!({ "type": "할일", "data": { "내용": "청소", "완료": false } }),
            ))
            .await
            .unwrap();

        let res = app
            .clone()
            .oneshot(post(
                "/api/pages/preview",
                json!({
                    "page": "미리보기",
                    "blocks": [
                        {
                            "view": "할 일",
                            "source": "할일",
                            "filter": { "완료": false },
                            "layout": "checklist"
                        }
                    ]
                }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = body_json(res).await;
        assert_eq!(body["page"], "미리보기");
        assert_eq!(body["blocks"][0]["entities"].as_array().unwrap().len(), 1);

        assert!(!vdir.join("미리보기.yaml").exists());
        let list = body_json(app.oneshot(get("/api/pages")).await.unwrap()).await;
        assert!(!list["pages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["page"] == "미리보기"));
    }

    #[tokio::test]
    async fn preview_없는_source는_400() {
        let (state, _dir) = test_state().await;
        let app = build_app(state);
        let res = app
            .oneshot(post(
                "/api/pages/preview",
                json!({ "page": "p", "blocks": [ { "view": "v", "source": "유령타입" } ] }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }
}
