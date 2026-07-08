use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use lifeops_core::entity::recurrence::{apply_recurrence, RecurrenceOutcome};
use lifeops_core::error::CoreError;
use lifeops_core::schema::FieldKind;
use lifeops_core::view::{is_system_column, matches_condition, resolve_today_token, sort_entities};
use serde_json::{json, Map, Value};
use std::collections::HashMap;

fn unknown_filter_field_error(ty: &str, field: &str) -> ApiError {
    ApiError(
        StatusCode::BAD_REQUEST,
        json!({ "error": { "code": "view", "message": format!("타입 '{ty}'에 없는 필터 필드 '{field}'") } }),
    )
}

const URL_OPS: [&str; 5] = ["lt", "gt", "lte", "gte", "month"];

/// "lte:$today+7d" → {"lte": "$today+7d"}, 그 외는 eq 스칼라. 숫자면 숫자로.
fn url_condition(raw: &str) -> Value {
    for op in URL_OPS {
        if let Some(v) = raw.strip_prefix(&format!("{op}:")) {
            return json!({ op: url_scalar(v) });
        }
    }
    url_scalar(raw)
}

fn url_scalar(s: &str) -> Value {
    s.parse::<f64>()
        .map(Value::from)
        .unwrap_or_else(|_| Value::String(s.to_string()))
}

fn bad_token_error(ty: &str, field: &str, raw: &str) -> ApiError {
    ApiError(
        StatusCode::BAD_REQUEST,
        json!({ "error": { "code": "view", "message": format!("타입 '{ty}' 필드 '{field}': 날짜 토큰 '{raw}'은 date 필드에서만, $today[±Nd] 형식만 지원") } }),
    )
}

/// GET /api/entities?type=물건&<필드>=<값>...&sort=[-]<필드>
///
/// `type`이 있는 경우: 스키마를 화이트리스트로 사용해 필터/정렬 필드를 검증하고
/// (뷰 엔진과 동일한 규칙으로) 정렬까지 적용하는 "자동 기본 뷰"로 동작한다.
/// `type`이 없는 경우(다중 타입 목록)는 검증할 단일 스키마가 없으므로
/// 필드 화이트리스트와 정렬을 적용하지 않는다(문자열 eq 필터만 유지).
pub async fn list(
    State(st): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let ty = params.get("type").cloned().unwrap_or_default();
    let schemas = st.schemas.read().await;

    if ty.is_empty() {
        // 타입 미지정 다중 타입 목록은 스키마 화이트리스트/정렬 미적용
        let types: Vec<String> = schemas.names().iter().map(|s| s.to_string()).collect();
        let mut entities = st.store.list(&types).await?;
        for (k, v) in &params {
            if k == "type" || k == "sort" {
                continue;
            }
            entities.retain(|e| e.data.get(k).and_then(Value::as_str) == Some(v.as_str()));
        }
        return Ok(Json(json!(entities)));
    }

    let schema = schemas
        .get(&ty)
        .ok_or_else(|| ApiError::from(CoreError::UnknownType(ty.clone())))?;

    let types = schemas.family_of(&ty);
    let mut entities = st.store.list(&types).await?;

    // type/sort 외의 쿼리 파라미터는 연산자(op:값) 또는 eq 필터, 스키마에 없는 필드는 400
    let today = chrono::Local::now().date_naive();
    for (k, v) in &params {
        if k == "type" || k == "sort" {
            continue;
        }
        let Some(fdef) = schema.fields.get(k) else {
            return Err(unknown_filter_field_error(&ty, k));
        };
        // $today 토큰 검증 (뷰 엔진과 동일 규칙)
        if v.contains("$today") {
            let token = v.rsplit(':').next().unwrap_or(v);
            if !matches!(fdef.kind, FieldKind::Date) || resolve_today_token(token, today).is_none() {
                return Err(bad_token_error(&ty, k, v));
            }
        }
        let condition = url_condition(v);
        entities.retain(|e| matches_condition(e.data.get(k), &fdef.kind, &condition, today));
    }

    if let Some(sort) = params.get("sort") {
        let field = sort.strip_prefix('-').unwrap_or(sort);
        if !is_system_column(field) && !schema.fields.contains_key(field) {
            return Err(unknown_filter_field_error(&ty, field));
        }
        sort_entities(&mut entities, schema, sort);
    }

    Ok(Json(json!(entities)))
}

/// POST /api/entities  { "type": "...", "data": { ... } }
pub async fn create(
    State(st): State<AppState>,
    Json(payload): Json<Value>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let ty = payload.get("type").and_then(Value::as_str).unwrap_or("").to_string();
    let data: Map<String, Value> = payload
        .get("data")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let schemas = st.schemas.read().await;
    let entity = st.store.create(&schemas, &ty, data).await?;
    Ok((StatusCode::CREATED, Json(json!(entity))))
}

/// GET /api/entities/:id → { entity, backlinks }
pub async fn get_one(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let entity = st.store.get(&id).await?;
    let Some(entity) = entity else {
        return Err(ApiError::from(lifeops_core::error::CoreError::NotFound(id)));
    };
    let backlinks = st.store.backlinks(&id).await?;
    let backlinks: Vec<_> = backlinks
        .iter()
        .map(|r| json!({ "from_id": r.from_id, "from_type": r.from_type, "field_name": r.field_name }))
        .collect();
    Ok(Json(json!({ "entity": entity, "backlinks": backlinks })))
}

/// PATCH /api/entities/:id  { <필드>: <값>, ... }
pub async fn update(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(patch): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let patch: Map<String, Value> = patch.as_object().cloned().unwrap_or_default();
    let schemas = st.schemas.read().await;
    let before = st
        .store
        .get(&id)
        .await?
        .ok_or_else(|| ApiError::from(CoreError::NotFound(id.clone())))?;
    let entity = st.store.update(&schemas, &id, patch).await?;
    let today = chrono::Local::now().date_naive();
    let outcome = apply_recurrence(&st.store, &schemas, &before, &entity, today).await?;
    let mut body = json!(entity);
    match outcome {
        RecurrenceOutcome::Spawned(spawned) => body["spawned"] = json!(spawned),
        RecurrenceOutcome::BadRule(rule) => {
            body["recurrence_warning"] = json!(format!("반복 규칙을 해석할 수 없음: {rule}"))
        }
        RecurrenceOutcome::NotApplicable => {}
    }
    Ok(Json(body))
}

/// DELETE /api/entities/:id
pub async fn delete(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    st.store.delete(&id).await?;
    Ok(StatusCode::NO_CONTENT)
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
    async fn 생성_조회_수정_목록_삭제_왕복() {
        let (state, _dir) = test_state().await;
        let app = build_app(state);

        // 생성
        let res = app.clone().oneshot(post("/api/entities",
            json!({ "type": "시계", "data": { "이름": "세이코 미쿠", "상태": "위시" } }))).await.unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);
        let created = body_json(res).await;
        let id = created["id"].as_str().unwrap().to_string();
        assert_eq!(created["type"], "시계");

        // 단건 조회 + 역링크 필드 존재
        let res = app.clone().oneshot(
            Request::builder().uri(format!("/api/entities/{id}")).body(Body::empty()).unwrap()
        ).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let got = body_json(res).await;
        assert_eq!(got["entity"]["data"]["이름"], "세이코 미쿠");
        assert!(got["backlinks"].is_array());

        // PATCH
        let res = app.clone().oneshot(
            Request::builder().method("PATCH").uri(format!("/api/entities/{id}"))
                .header("content-type", "application/json")
                .body(Body::from(json!({ "상태": "주문됨" }).to_string())).unwrap()
        ).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(body_json(res).await["data"]["상태"], "주문됨");

        // 목록(type=물건 → 시계 포함)
        let res = app.clone().oneshot(
            Request::builder().uri("/api/entities?type=물건").body(Body::empty()).unwrap()
        ).await.unwrap();
        let list = body_json(res).await;
        assert_eq!(list.as_array().unwrap().len(), 1);

        // 삭제
        let res = app.clone().oneshot(
            Request::builder().method("DELETE").uri(format!("/api/entities/{id}")).body(Body::empty()).unwrap()
        ).await.unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn 검증_실패는_400_필드에러() {
        let (state, _dir) = test_state().await;
        let app = build_app(state);
        let res = app.oneshot(post("/api/entities",
            json!({ "type": "시계", "data": { "상태": "위시" } }))).await.unwrap(); // 이름(필수) 누락
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let body = body_json(res).await;
        assert_eq!(body["error"]["code"], "validation");
        assert!(body["error"]["fields"].as_array().unwrap().iter().any(|f| f["field"] == "이름"));
    }

    #[tokio::test]
    async fn 싱글턴_중복_생성은_409() {
        let (state, _dir) = test_state().await;
        std::fs::write(
            state.schemas_dir.join("프로필.yaml"),
            "type: 프로필\nsingleton: true\nfields:\n  이름: { kind: text }\n",
        )
        .unwrap();
        let app = build_app(state);
        app.clone().oneshot(post("/api/reload", json!({}))).await.unwrap();

        let first = app.clone().oneshot(post("/api/entities",
            json!({ "type": "프로필", "data": { "이름": "미쿠" } }))).await.unwrap();
        assert_eq!(first.status(), StatusCode::CREATED);

        let second = app.oneshot(post("/api/entities",
            json!({ "type": "프로필", "data": { "이름": "린" } }))).await.unwrap();
        assert_eq!(second.status(), StatusCode::CONFLICT);
        let body = body_json(second).await;
        assert_eq!(body["error"]["code"], "singleton_exists");
    }

    #[tokio::test]
    async fn 삭제_차단은_409() {
        let (state, _dir) = test_state().await;
        let app = build_app(state);
        let w = body_json(app.clone().oneshot(post("/api/entities",
            json!({ "type": "시계", "data": { "이름": "미쿠" } }))).await.unwrap()).await;
        let wid = w["id"].as_str().unwrap();
        app.clone().oneshot(post("/api/entities",
            json!({ "type": "할일", "data": { "내용": "개봉", "관련물건": [wid] } }))).await.unwrap();
        let res = app.oneshot(
            Request::builder().method("DELETE").uri(format!("/api/entities/{wid}")).body(Body::empty()).unwrap()
        ).await.unwrap();
        assert_eq!(res.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn 목록_sort_적용됨() {
        let (state, _dir) = test_state().await;
        let app = build_app(state);
        for (이름, 가격) in [("A", 300000.0), ("B", 100000.0), ("C", 650000.0)] {
            app.clone().oneshot(post("/api/entities", json!({
                "type": "시계",
                "data": { "이름": 이름, "가격": { "amount": 가격, "currency": "KRW" } }
            }))).await.unwrap();
        }

        let res = app.oneshot(
            Request::builder().uri("/api/entities?type=물건&sort=-가격").body(Body::empty()).unwrap()
        ).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let list = body_json(res).await;
        let names: Vec<&str> = list.as_array().unwrap().iter()
            .map(|e| e["data"]["이름"].as_str().unwrap()).collect();
        assert_eq!(names, ["C", "A", "B"]);
    }

    #[tokio::test]
    async fn 목록_없는_필터필드는_400() {
        let (state, _dir) = test_state().await;
        let app = build_app(state);
        let res = app.oneshot(
            Request::builder().uri("/api/entities?type=물건&유령=x").body(Body::empty()).unwrap()
        ).await.unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let body = body_json(res).await;
        assert_eq!(body["error"]["code"], "view");
    }

    #[tokio::test]
    async fn 목록_없는_타입은_404() {
        let (state, _dir) = test_state().await;
        let app = build_app(state);
        let res = app.oneshot(
            Request::builder().uri("/api/entities?type=유령타입").body(Body::empty()).unwrap()
        ).await.unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn 목록_연산자_필터와_today_토큰() {
        let (state, _dir) = test_state().await;
        let app = build_app(state);
        for (이름, 가격) in [("싼것", 100000.0), ("비싼것", 650000.0)] {
            app.clone().oneshot(post("/api/entities", json!({
                "type": "시계", "data": { "이름": 이름, "가격": { "amount": 가격, "currency": "KRW" } }
            }))).await.unwrap();
        }
        let res = app.clone().oneshot(
            Request::builder().uri("/api/entities?type=물건&%EA%B0%80%EA%B2%A9=gte:200000").body(Body::empty()).unwrap()
        ).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let list = body_json(res).await;
        assert_eq!(list.as_array().unwrap().len(), 1);
        assert_eq!(list[0]["data"]["이름"], "비싼것");

        // 비date 필드에 $today → 400
        let res = app.oneshot(
            Request::builder().uri("/api/entities?type=물건&%EC%9D%B4%EB%A6%84=lte:$today").body(Body::empty()).unwrap()
        ).await.unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn 목록_시스템컬럼_정렬_허용() {
        let (state, _dir) = test_state().await;
        let app = build_app(state);
        let res = app.oneshot(
            Request::builder().uri("/api/entities?type=물건&sort=-updated_at").body(Body::empty()).unwrap()
        ).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn 반복_할일_완료시_spawned_포함() {
        let (state, _dir) = test_state().await;
        let app = build_app(state);
        let created = body_json(app.clone().oneshot(post("/api/entities",
            json!({ "type": "할일", "data": { "내용": "청소", "완료": false, "마감일": "2026-01-05", "반복": "매주" } }))).await.unwrap()).await;
        let id = created["id"].as_str().unwrap();

        let res = app.clone().oneshot(
            Request::builder().method("PATCH").uri(format!("/api/entities/{id}"))
                .header("content-type", "application/json")
                .body(Body::from(json!({ "완료": true }).to_string())).unwrap()
        ).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = body_json(res).await;
        assert_eq!(body["data"]["완료"], json!(true));
        let spawned = &body["spawned"];
        assert_eq!(spawned["data"]["완료"], json!(false));
        assert!(spawned["data"]["마감일"].as_str().unwrap() > "2026-01-05");
    }

    #[tokio::test]
    async fn 잘못된_반복은_경고만() {
        let (state, _dir) = test_state().await;
        let app = build_app(state);
        let created = body_json(app.clone().oneshot(post("/api/entities",
            json!({ "type": "할일", "data": { "내용": "x", "완료": false, "반복": "격주" } }))).await.unwrap()).await;
        let id = created["id"].as_str().unwrap();
        let res = app.oneshot(
            Request::builder().method("PATCH").uri(format!("/api/entities/{id}"))
                .header("content-type", "application/json")
                .body(Body::from(json!({ "완료": true }).to_string())).unwrap()
        ).await.unwrap();
        let body = body_json(res).await;
        assert_eq!(body["data"]["완료"], json!(true)); // 완료는 진행
        assert!(body.get("spawned").is_none());
        assert!(body["recurrence_warning"].as_str().unwrap().contains("격주"));
    }
}
