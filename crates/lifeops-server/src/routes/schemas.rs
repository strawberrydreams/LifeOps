use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use indexmap::IndexMap;
use lifeops_core::error::SchemaError;
use lifeops_core::schema::{
    load_raw_dir, to_yaml, FieldKind, RawBehaviors, RawFieldDef, RawSchema, SchemaSet,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static SCHEMA_TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Deserialize)]
pub struct SchemaInput {
    #[serde(rename = "type", default)]
    pub name: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub extends: Option<String>,
    #[serde(default)]
    pub behaviors: Option<RawBehaviors>,
    #[serde(default)]
    pub fields: IndexMap<String, RawFieldDef>,
    #[serde(default)]
    pub field_order: Option<Vec<String>>,
    #[serde(default)]
    pub singleton: Option<bool>,
    #[serde(default)]
    pub renames: IndexMap<String, String>,
}

#[derive(Deserialize)]
pub struct UpdateQuery {
    #[serde(default)]
    pub dry_run: bool,
}

pub async fn create(
    State(st): State<AppState>,
    Json(input): Json<SchemaInput>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let name = input
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| {
            ApiError(
                StatusCode::BAD_REQUEST,
                json!({ "error": { "code": "bad_schema_name", "message": "type은 필수입니다" } }),
            )
        })?
        .to_string();
    let filename = safe_filename(&name)?;
    let raw = to_raw_schema(name.clone(), &input, false);
    apply_and_persist(&st, &name, &filename, raw).await?;

    Ok((StatusCode::CREATED, Json(json!({ "ok": true }))))
}

pub async fn get_one(
    State(st): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let raw = load_raw_dir(&st.schemas_dir).map_err(schema_error_to_api)?;
    let Some((own, _)) = raw.get(&name) else {
        return Err(ApiError(
            StatusCode::NOT_FOUND,
            json!({ "error": { "code": "unknown_type", "message": format!("알 수 없는 타입 '{name}'") } }),
        ));
    };

    let schemas = st.schemas.read().await;
    let mut inherited = Map::new();
    if let Some(resolved) = schemas.get(&name) {
        for (field_name, field) in &resolved.fields {
            if !own.fields.contains_key(field_name) {
                inherited.insert(field_name.clone(), json!(field));
            }
        }
    }

    Ok(Json(json!({
        "type": own.name,
        "category": own.category,
        "singleton": own.singleton,
        "extends": own.extends,
        "behaviors": own.behaviors,
        "field_order": own.field_order,
        "fields": own.fields,
        "inherited": inherited,
    })))
}

pub async fn update(
    State(st): State<AppState>,
    Path(name): Path<String>,
    Query(query): Query<UpdateQuery>,
    Json(input): Json<SchemaInput>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let filename = safe_filename(&name)?;
    let result = update_schema(&st, &name, &filename, input, query.dry_run).await?;
    if query.dry_run {
        Ok(Json(json!({
            "affected_entities": result.affected_entities,
            "warnings": result.warnings
        })))
    } else {
        Ok(Json(json!({ "ok": true })))
    }
}

pub async fn delete(
    State(st): State<AppState>,
    Path(name): Path<String>,
) -> Result<StatusCode, ApiError> {
    safe_filename(&name)?;
    delete_schema(&st, &name).await?;
    Ok(StatusCode::NO_CONTENT)
}

fn to_raw_schema(name: String, input: &SchemaInput, prior_singleton: bool) -> RawSchema {
    let _ = &input.renames;
    RawSchema {
        name,
        extends: input.extends.clone(),
        category: input.category.clone(),
        singleton: input.singleton.unwrap_or(prior_singleton),
        behaviors: input.behaviors.clone(),
        fields: input.fields.clone(),
        field_order: input.field_order.clone(),
    }
}

fn safe_filename(name: &str) -> Result<String, ApiError> {
    if name.trim().is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            json!({ "error": { "code": "bad_schema_name", "message": "안전하지 않은 타입명입니다" } }),
        ));
    }
    Ok(format!("{name}.yaml"))
}

fn schema_error_to_api(err: SchemaError) -> ApiError {
    match err {
        SchemaError::Io(_) | SchemaError::Parse { .. } | SchemaError::DuplicateType { .. } => {
            tracing::error!("스키마 로드 오류: {err}");
            ApiError(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({ "error": { "code": "schema_load", "message": "내부 서버 오류" } }),
            )
        }
        SchemaError::UnknownParent { .. }
        | SchemaError::Cycle { .. }
        | SchemaError::BadKind { .. }
        | SchemaError::EnumWithoutOptions { .. }
        | SchemaError::UnknownRefTarget { .. }
        | SchemaError::UnknownFieldInOrder { .. }
        | SchemaError::BadBehavior { .. } => ApiError(
            StatusCode::BAD_REQUEST,
            json!({ "error": { "code": "schema_validation", "message": err.to_string() } }),
        ),
    }
}

async fn apply_and_persist(
    st: &AppState,
    name: &str,
    filename: &str,
    raw: RawSchema,
) -> Result<(), ApiError> {
    let mut schemas = st.schemas.write().await;
    if schemas.get(name).is_some() {
        return Err(ApiError(
            StatusCode::CONFLICT,
            json!({ "error": { "code": "schema_exists", "message": format!("이미 존재하는 타입입니다: {name}") } }),
        ));
    }

    let schemas_dir = st.schemas_dir.as_path();
    let mut candidates = load_raw_dir(schemas_dir).map_err(schema_error_to_api)?;
    if candidates.contains_key(name) || schemas_dir.join(filename).exists() {
        return Err(ApiError(
            StatusCode::CONFLICT,
            json!({ "error": { "code": "schema_exists", "message": format!("이미 존재하는 타입입니다: {name}") } }),
        ));
    }

    candidates.insert(name.to_string(), (raw.clone(), filename.to_string()));
    let new_set = SchemaSet::from_raw(&candidates).map_err(schema_error_to_api)?;
    let yaml = to_yaml(&raw).map_err(|err| {
        tracing::error!("스키마 직렬화 오류: {err}");
        ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({ "error": { "code": "schema_serialize", "message": "내부 서버 오류" } }),
        )
    })?;

    let tmp_path = schemas_dir.join(unique_tmp_filename(filename));
    let final_path = schemas_dir.join(filename);
    std::fs::write(&tmp_path, yaml).map_err(|err| io_api_error("schema_write", err))?;
    if let Err(err) = std::fs::rename(&tmp_path, &final_path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(io_api_error("schema_write", err));
    }

    *schemas = new_set;
    Ok(())
}

struct UpdateResult {
    affected_entities: usize,
    warnings: Vec<String>,
}

async fn update_schema(
    st: &AppState,
    name: &str,
    filename: &str,
    input: SchemaInput,
    dry_run: bool,
) -> Result<UpdateResult, ApiError> {
    let mut schemas = st.schemas.write().await;
    if schemas.get(name).is_none() {
        return Err(ApiError(
            StatusCode::NOT_FOUND,
            json!({ "error": { "code": "unknown_type", "message": format!("알 수 없는 타입 '{name}'") } }),
        ));
    }
    let family = schemas.family_of(name);
    let mut candidates = load_raw_dir(&st.schemas_dir).map_err(schema_error_to_api)?;
    let Some((existing_raw, existing_filename)) = candidates.get(name).cloned() else {
        tracing::error!("메모리에는 있지만 파일에는 없는 스키마 타입: {name}");
        return Err(ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({ "error": { "code": "schema_load", "message": "내부 서버 오류" } }),
        ));
    };

    validate_immutable_update(name, &input, &existing_raw)?;
    validate_update_renames(&existing_raw, &input)?;

    let raw = to_raw_schema(name.to_string(), &input, existing_raw.singleton);
    candidates.insert(name.to_string(), (raw.clone(), existing_filename.clone()));
    let new_set = SchemaSet::from_raw(&candidates).map_err(schema_error_to_api)?;

    let impact_changes = impact_changes(&existing_raw, &raw, &input.renames);
    let impact_report = compute_impact_report(st, &family, &impact_changes).await?;

    if dry_run {
        return Ok(UpdateResult {
            affected_entities: impact_report.affected_entities,
            warnings: impact_report.warnings,
        });
    }

    let yaml = to_yaml(&raw).map_err(|err| {
        tracing::error!("스키마 직렬화 오류: {err}");
        ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({ "error": { "code": "schema_serialize", "message": "내부 서버 오류" } }),
        )
    })?;

    let renames: Vec<_> = input
        .renames
        .iter()
        .map(|(old, new)| (old.clone(), new.clone()))
        .collect();
    let inverse_renames: Vec<_> = renames
        .iter()
        .map(|(old, new)| (new.clone(), old.clone()))
        .collect();
    if !renames.is_empty() {
        st.store.rename_field(&family, &renames).await?;
    }

    let tmp_path = st.schemas_dir.join(unique_tmp_filename(filename));
    let final_path = st.schemas_dir.join(existing_filename);
    let write_result = std::fs::write(&tmp_path, yaml)
        .map_err(|err| io_api_error("schema_write", err))
        .and_then(|_| {
            if let Err(err) = std::fs::rename(&tmp_path, &final_path) {
                let _ = std::fs::remove_file(&tmp_path);
                return Err(io_api_error("schema_write", err));
            }
            Ok(())
        });

    if let Err(err) = write_result {
        if !inverse_renames.is_empty() {
            if let Err(rollback_err) = st.store.rename_field(&family, &inverse_renames).await {
                tracing::error!("스키마 저장 실패 후 필드 rename rollback 실패: {rollback_err}");
            }
        }
        return Err(err);
    }

    *schemas = new_set;
    Ok(UpdateResult {
        affected_entities: impact_report.affected_entities,
        warnings: impact_report.warnings,
    })
}

async fn delete_schema(st: &AppState, name: &str) -> Result<(), ApiError> {
    let mut schemas = st.schemas.write().await;
    if schemas.get(name).is_none() {
        return Err(ApiError(
            StatusCode::NOT_FOUND,
            json!({ "error": { "code": "unknown_type", "message": format!("알 수 없는 타입 '{name}'") } }),
        ));
    }

    let entities = st.store.list(&[name.to_string()]).await?;
    if !entities.is_empty() {
        return Err(ApiError(
            StatusCode::CONFLICT,
            json!({ "error": { "code": "schema_in_use", "message": format!("타입 '{name}'의 엔티티가 있어 삭제할 수 없습니다") } }),
        ));
    }

    let mut candidates = load_raw_dir(&st.schemas_dir).map_err(schema_error_to_api)?;
    let Some((_, filename)) = candidates.get(name).cloned() else {
        tracing::error!("메모리에는 있지만 파일에는 없는 스키마 타입: {name}");
        return Err(ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({ "error": { "code": "schema_load", "message": "내부 서버 오류" } }),
        ));
    };

    validate_delete_references(name, &candidates)?;

    candidates.shift_remove(name);
    let new_set = SchemaSet::from_raw(&candidates).map_err(schema_error_to_api)?;
    std::fs::remove_file(st.schemas_dir.join(filename))
        .map_err(|err| io_api_error("schema_delete", err))?;

    *schemas = new_set;
    Ok(())
}

fn validate_delete_references(
    name: &str,
    candidates: &IndexMap<String, (RawSchema, String)>,
) -> Result<(), ApiError> {
    for (other_name, (schema, _)) in candidates {
        if other_name == name {
            continue;
        }
        if schema.extends.as_deref() == Some(name) {
            return Err(ApiError(
                StatusCode::CONFLICT,
                json!({ "error": { "code": "schema_referenced", "message": format!("타입 '{other_name}'이 '{name}'을 extends하여 삭제할 수 없습니다") } }),
            ));
        }
        for (field_name, field) in &schema.fields {
            if field.target.as_deref() == Some(name)
                && FieldKind::parse(&field.kind).is_some_and(|kind| kind.contains_ref())
            {
                return Err(ApiError(
                    StatusCode::CONFLICT,
                    json!({ "error": { "code": "schema_referenced", "message": format!("타입 '{other_name}'의 필드 '{field_name}'가 '{name}'을 참조하여 삭제할 수 없습니다") } }),
                ));
            }
        }
    }
    Ok(())
}

fn validate_immutable_update(
    name: &str,
    input: &SchemaInput,
    existing_raw: &RawSchema,
) -> Result<(), ApiError> {
    if let Some(body_name) = input.name.as_deref().map(str::trim) {
        if body_name != name {
            return Err(schema_immutable_error("type은 생성 후 변경할 수 없습니다"));
        }
    }
    if input.extends != existing_raw.extends {
        return Err(schema_immutable_error(
            "extends는 생성 후 변경할 수 없습니다",
        ));
    }
    Ok(())
}

fn validate_update_renames(existing_raw: &RawSchema, input: &SchemaInput) -> Result<(), ApiError> {
    let old_names: HashSet<_> = input.renames.keys().map(String::as_str).collect();
    let mut new_names = HashSet::new();
    for (old, new) in &input.renames {
        if old == new {
            return Err(schema_validation_error(format!(
                "필드 이름 변경 '{old}' -> '{new}'는 같은 이름입니다"
            )));
        }
        if !existing_raw.fields.contains_key(old) {
            return Err(schema_validation_error(format!(
                "필드 이름 변경 old '{old}'가 기존 스키마에 없습니다"
            )));
        }
        if input.fields.contains_key(old) {
            return Err(schema_validation_error(format!(
                "필드 이름 변경 source '{old}'는 새 스키마 fields에서 제거되어야 합니다"
            )));
        }
        if existing_raw.fields.contains_key(new) {
            return Err(schema_validation_error(format!(
                "필드 이름 변경 target '{new}'가 기존 스키마 필드와 충돌합니다"
            )));
        }
        if !input.fields.contains_key(new) {
            return Err(schema_validation_error(format!(
                "필드 이름 변경 new '{new}'가 새 스키마에 없습니다"
            )));
        }
        if !new_names.insert(new.as_str()) {
            return Err(schema_validation_error(format!(
                "필드 이름 변경 new '{new}'가 중복됨"
            )));
        }
        if old_names.contains(new.as_str()) {
            return Err(schema_validation_error(format!(
                "필드 이름 변경 '{old}' -> '{new}'가 다른 변경의 old와 겹침"
            )));
        }
    }
    Ok(())
}

struct ImpactReport {
    affected_entities: usize,
    warnings: Vec<String>,
}

struct CountedImpact {
    change: ImpactChange,
    count: usize,
}

#[derive(Clone)]
enum ImpactChange {
    Removed {
        field: String,
    },
    Renamed {
        old: String,
        new: String,
    },
    KindChanged {
        field: String,
        old_kind: String,
        new_kind: String,
    },
}

impl ImpactChange {
    fn source_field(&self) -> &str {
        match self {
            ImpactChange::Removed { field } | ImpactChange::KindChanged { field, .. } => {
                field.as_str()
            }
            ImpactChange::Renamed { old, .. } => old.as_str(),
        }
    }

    fn warning(&self, count: usize) -> String {
        match self {
            ImpactChange::Removed { field } => {
                format!("필드 '{field}' 제거는 기존 엔티티 {count}건의 데이터에 영향을 줍니다")
            }
            ImpactChange::Renamed { old, new } => {
                format!("필드 '{old}' -> '{new}' 이름 변경은 기존 엔티티 {count}건의 데이터 키를 바꿉니다")
            }
            ImpactChange::KindChanged {
                field,
                old_kind,
                new_kind,
            } => {
                format!(
                    "필드 '{field}' kind 변경({old_kind} -> {new_kind})은 기존 엔티티 {count}건의 데이터에 영향을 줍니다"
                )
            }
        }
    }
}

fn impact_changes(
    existing_raw: &RawSchema,
    raw: &RawSchema,
    renames: &IndexMap<String, String>,
) -> Vec<ImpactChange> {
    let mut changes = Vec::new();
    for field in existing_raw.fields.keys() {
        if !raw.fields.contains_key(field) && !renames.contains_key(field) {
            changes.push(ImpactChange::Removed {
                field: field.clone(),
            });
        }
    }
    for (old, new) in renames {
        changes.push(ImpactChange::Renamed {
            old: old.clone(),
            new: new.clone(),
        });
        if let (Some(existing_field), Some(new_field)) =
            (existing_raw.fields.get(old), raw.fields.get(new))
        {
            if existing_field.kind != new_field.kind {
                changes.push(ImpactChange::KindChanged {
                    field: new.clone(),
                    old_kind: existing_field.kind.clone(),
                    new_kind: new_field.kind.clone(),
                });
            }
        }
    }
    for (field, existing_field) in &existing_raw.fields {
        let Some(new_field) = raw.fields.get(field) else {
            continue;
        };
        if existing_field.kind != new_field.kind {
            changes.push(ImpactChange::KindChanged {
                field: field.clone(),
                old_kind: existing_field.kind.clone(),
                new_kind: new_field.kind.clone(),
            });
        }
    }
    changes
}

async fn compute_impact_report(
    st: &AppState,
    family: &[String],
    changes: &[ImpactChange],
) -> Result<ImpactReport, ApiError> {
    if changes.is_empty() {
        return Ok(ImpactReport {
            affected_entities: 0,
            warnings: Vec::new(),
        });
    }
    let entities = st.store.list(family).await?;
    let mut counted: Vec<_> = changes
        .iter()
        .cloned()
        .map(|change| CountedImpact { change, count: 0 })
        .collect();
    let mut affected_entities = 0;
    for entity in &entities {
        let mut affected = false;
        for impact in &mut counted {
            if has_non_null_value(&entity.data, impact.change.source_field()) {
                impact.count += 1;
                affected = true;
            }
        }
        if affected {
            affected_entities += 1;
        }
    }
    let warnings = counted
        .iter()
        .map(|impact| impact.change.warning(impact.count))
        .collect();
    Ok(ImpactReport {
        affected_entities,
        warnings,
    })
}

fn has_non_null_value(data: &Map<String, Value>, field: &str) -> bool {
    data.get(field).is_some_and(|value| !value.is_null())
}

fn schema_immutable_error(message: &str) -> ApiError {
    ApiError(
        StatusCode::BAD_REQUEST,
        json!({ "error": { "code": "schema_immutable", "message": message } }),
    )
}

fn schema_validation_error(message: String) -> ApiError {
    ApiError(
        StatusCode::BAD_REQUEST,
        json!({ "error": { "code": "schema_validation", "message": message } }),
    )
}

fn unique_tmp_filename(filename: &str) -> String {
    let seq = SCHEMA_TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{filename}.{}.{}.{}.tmp", std::process::id(), nanos, seq)
}

fn io_api_error(code: &str, err: std::io::Error) -> ApiError {
    tracing::error!("스키마 파일 쓰기 오류: {err}");
    ApiError(
        StatusCode::INTERNAL_SERVER_ERROR,
        json!({ "error": { "code": code, "message": "내부 서버 오류" } }),
    )
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
    async fn 편집은_body에_없어도_singleton을_보존한다() {
        let (state, _dir) = test_state().await;
        std::fs::write(
            state.schemas_dir.join("프로필.yaml"),
            "type: 프로필\nsingleton: true\nfields:\n  이름: { kind: text }\n",
        )
        .unwrap();
        let app = build_app(state.clone());
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/reload")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let res = app
            .clone()
            .oneshot(put(
                "/api/schemas/프로필",
                json!({
                    "type": "프로필",
                    "fields": {
                        "이름": { "kind": "text" },
                        "거주지": { "kind": "text" }
                    }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/schemas/프로필")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = body_json(res).await;
        assert_eq!(body["singleton"], json!(true));
    }

    #[tokio::test]
    async fn 새_타입_생성후_schemas에_반영된다() {
        let (state, dir) = test_state().await;
        let sdir = state.schemas_dir.clone();
        let app = build_app(state);
        let res = app
            .clone()
            .oneshot(post(
                "/api/schemas",
                json!({
                    "type": "북마크", "category": "메모",
                    "fields": { "제목": { "kind": "text", "required": true }, "링크": { "kind": "url" } }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);

        assert!(sdir.join("북마크.yaml").exists());
        let body = body_json(
            app.oneshot(
                Request::builder()
                    .uri("/api/schemas")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
        )
        .await;
        assert!(body["types"].get("북마크").is_some());
        assert_eq!(
            body["types"]["북마크"]["fields"]["제목"]["required"],
            json!(true)
        );
        drop(dir);
    }

    #[tokio::test]
    async fn 잘못된_kind는_400이고_파일을_남기지_않는다() {
        let (state, _dir) = test_state().await;
        let sdir = state.schemas_dir.clone();
        let app = build_app(state);
        let res = app
            .oneshot(post(
                "/api/schemas",
                json!({
                    "type": "깨진것", "fields": { "x": { "kind": "geo" } }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        assert!(
            !sdir.join("깨진것.yaml").exists(),
            "실패 시 파일이 없어야 함"
        );
    }

    #[tokio::test]
    async fn 알수없는_ref_target은_400이고_파일을_남기지_않는다() {
        let (state, _dir) = test_state().await;
        let sdir = state.schemas_dir.clone();
        let app = build_app(state);
        let res = app
            .oneshot(post(
                "/api/schemas",
                json!({
                    "type": "방문", "fields": { "곳": { "kind": "ref", "target": "유령" } }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        assert!(!sdir.join("방문.yaml").exists());
    }

    #[tokio::test]
    async fn 이미_있는_타입_생성은_409() {
        let (state, _dir) = test_state().await;
        let app = build_app(state);
        let res = app
            .oneshot(post(
                "/api/schemas",
                json!({
                    "type": "물건", "fields": { "이름": { "kind": "text" } }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn 동시_다른_타입_생성은_둘다_schemas에_반영된다() {
        let (state, _dir) = test_state().await;
        let sdir = state.schemas_dir.clone();
        let app = build_app(state);

        let (one, two) = tokio::join!(
            app.clone().oneshot(post(
                "/api/schemas",
                json!({
                    "type": "책갈피", "fields": { "제목": { "kind": "text" } }
                }),
            )),
            app.clone().oneshot(post(
                "/api/schemas",
                json!({
                    "type": "레시피", "fields": { "이름": { "kind": "text" } }
                }),
            )),
        );

        let one = one.unwrap();
        let two = two.unwrap();
        assert_eq!(one.status(), StatusCode::CREATED);
        assert_eq!(two.status(), StatusCode::CREATED);
        assert!(sdir.join("책갈피.yaml").exists());
        assert!(sdir.join("레시피.yaml").exists());

        let body = body_json(
            app.oneshot(
                Request::builder()
                    .uri("/api/schemas")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
        )
        .await;
        assert!(body["types"].get("책갈피").is_some());
        assert!(body["types"].get("레시피").is_some());
    }

    #[tokio::test]
    async fn 동시_같은_타입_생성은_하나만_성공한다() {
        let (state, _dir) = test_state().await;
        let app = build_app(state);

        let (one, two) = tokio::join!(
            app.clone().oneshot(post(
                "/api/schemas",
                json!({
                    "type": "중복타입", "fields": { "제목": { "kind": "text" } }
                }),
            )),
            app.clone().oneshot(post(
                "/api/schemas",
                json!({
                    "type": "중복타입", "fields": { "제목": { "kind": "text" } }
                }),
            )),
        );

        let mut statuses = vec![one.unwrap().status(), two.unwrap().status()];
        statuses.sort();
        assert_eq!(statuses, [StatusCode::CREATED, StatusCode::CONFLICT]);
    }

    #[tokio::test]
    async fn 필드_추가_수정이_반영된다() {
        let (state, _dir) = test_state().await;
        let app = build_app(state);

        let res = app
            .clone()
            .oneshot(put(
                "/api/schemas/물건",
                json!({
                    "type": "물건",
                    "category": "컬렉션",
                    "fields": {
                        "이름": { "kind": "text", "required": true },
                        "상태": { "kind": "enum", "options": ["위시", "주문됨", "보유", "과거"] },
                        "가격": { "kind": "money" },
                        "메모": { "kind": "text" }
                    }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let body = body_json(
            app.oneshot(
                Request::builder()
                    .uri("/api/schemas")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
        )
        .await;
        assert!(body["types"]["물건"]["fields"].get("메모").is_some());
        assert!(body["types"]["물건"]["fields"].get("가격").is_some());
    }

    #[tokio::test]
    async fn dry_run은_저장하지_않고_영향을_보고한다() {
        let (state, _dir) = test_state().await;
        let sdir = state.schemas_dir.clone();
        let app = build_app(state);

        app.clone()
            .oneshot(post(
                "/api/entities",
                json!({
                    "type": "물건",
                    "data": { "이름": "잡화", "가격": { "amount": 12000, "currency": "KRW" } }
                }),
            ))
            .await
            .unwrap();

        let res = app
            .oneshot(put(
                "/api/schemas/물건?dry_run=true",
                json!({
                    "type": "물건",
                    "category": "컬렉션",
                    "fields": {
                        "이름": { "kind": "text", "required": true },
                        "상태": { "kind": "enum", "options": ["위시", "주문됨", "보유", "과거"] }
                    }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = body_json(res).await;
        assert_eq!(body["affected_entities"], json!(1));
        let warnings = body["warnings"].as_array().unwrap();
        assert!(warnings
            .iter()
            .any(|w| w.as_str().unwrap().contains("가격")));

        let saved = std::fs::read_to_string(sdir.join("물건.yaml")).unwrap();
        assert!(saved.contains("가격"));
    }

    #[tokio::test]
    async fn dry_run은_null_값을_영향_대상에서_제외한다() {
        let (state, _dir) = test_state().await;
        let app = build_app(state);

        app.clone()
            .oneshot(post(
                "/api/entities",
                json!({
                    "type": "물건",
                    "data": { "이름": "빈가격", "가격": null }
                }),
            ))
            .await
            .unwrap();
        app.clone()
            .oneshot(post(
                "/api/entities",
                json!({
                    "type": "물건",
                    "data": { "이름": "실가격", "가격": { "amount": 12000, "currency": "KRW" } }
                }),
            ))
            .await
            .unwrap();

        let res = app
            .oneshot(put(
                "/api/schemas/물건?dry_run=true",
                json!({
                    "type": "물건",
                    "category": "컬렉션",
                    "fields": {
                        "이름": { "kind": "text", "required": true },
                        "상태": { "kind": "enum", "options": ["위시", "주문됨", "보유", "과거"] }
                    }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = body_json(res).await;
        assert_eq!(body["affected_entities"], json!(1));
        assert!(body["warnings"][0].as_str().unwrap().contains("1"));
    }

    #[tokio::test]
    async fn dry_run_rename은_필드명과_건수_warning을_반환한다() {
        let (state, _dir) = test_state().await;
        let app = build_app(state);

        app.clone()
            .oneshot(post(
                "/api/entities",
                json!({
                    "type": "물건",
                    "data": { "이름": "잡화" }
                }),
            ))
            .await
            .unwrap();

        let res = app
            .oneshot(put(
                "/api/schemas/물건?dry_run=true",
                json!({
                    "type": "물건",
                    "category": "컬렉션",
                    "fields": {
                        "명칭": { "kind": "text", "required": true },
                        "상태": { "kind": "enum", "options": ["위시", "주문됨", "보유", "과거"] },
                        "가격": { "kind": "money" }
                    },
                    "renames": { "이름": "명칭" }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = body_json(res).await;
        assert_eq!(body["affected_entities"], json!(1));
        let warnings = body["warnings"].as_array().unwrap();
        assert!(warnings.iter().any(|w| {
            let w = w.as_str().unwrap();
            w.contains("이름") && w.contains("명칭") && w.contains("1")
        }));
    }

    #[tokio::test]
    async fn dry_run_rename과_kind변경이_같이_있으면_둘다_warning을_반환한다() {
        let (state, _dir) = test_state().await;
        let app = build_app(state);

        app.clone()
            .oneshot(post(
                "/api/entities",
                json!({
                    "type": "물건",
                    "data": { "이름": "잡화", "가격": { "amount": 12000, "currency": "KRW" } }
                }),
            ))
            .await
            .unwrap();

        let res = app
            .oneshot(put(
                "/api/schemas/물건?dry_run=true",
                json!({
                    "type": "물건",
                    "category": "컬렉션",
                    "fields": {
                        "이름": { "kind": "text", "required": true },
                        "상태": { "kind": "enum", "options": ["위시", "주문됨", "보유", "과거"] },
                        "비용": { "kind": "text" }
                    },
                    "renames": { "가격": "비용" }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = body_json(res).await;
        assert_eq!(body["affected_entities"], json!(1));
        let warnings = body["warnings"].as_array().unwrap();
        assert!(warnings.iter().any(|w| {
            let w = w.as_str().unwrap();
            w.contains("가격") && w.contains("비용") && w.contains("이름 변경")
        }));
        assert!(warnings.iter().any(|w| {
            let w = w.as_str().unwrap();
            w.contains("비용") && w.contains("kind") && w.contains("money") && w.contains("text")
        }));
    }

    #[tokio::test]
    async fn dry_run_kind_변경은_영향과_warning을_반환한다() {
        let (state, _dir) = test_state().await;
        let app = build_app(state);

        app.clone()
            .oneshot(post(
                "/api/entities",
                json!({
                    "type": "물건",
                    "data": { "이름": "빈가격", "가격": null }
                }),
            ))
            .await
            .unwrap();
        app.clone()
            .oneshot(post(
                "/api/entities",
                json!({
                    "type": "물건",
                    "data": { "이름": "실가격", "가격": { "amount": 12000, "currency": "KRW" } }
                }),
            ))
            .await
            .unwrap();

        let res = app
            .oneshot(put(
                "/api/schemas/물건?dry_run=true",
                json!({
                    "type": "물건",
                    "category": "컬렉션",
                    "fields": {
                        "이름": { "kind": "text", "required": true },
                        "상태": { "kind": "enum", "options": ["위시", "주문됨", "보유", "과거"] },
                        "가격": { "kind": "text" }
                    }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = body_json(res).await;
        assert_eq!(body["affected_entities"], json!(1));
        let warnings = body["warnings"].as_array().unwrap();
        assert!(warnings.iter().any(|w| {
            let w = w.as_str().unwrap();
            w.contains("가격") && w.contains("kind") && w.contains("money") && w.contains("text")
        }));
    }

    #[tokio::test]
    async fn rename은_엔티티_데이터_키를_바꾼다() {
        let (state, _dir) = test_state().await;
        let app = build_app(state);

        let created = body_json(
            app.clone()
                .oneshot(post(
                    "/api/entities",
                    json!({
                        "type": "물건",
                        "data": { "이름": "잡화" }
                    }),
                ))
                .await
                .unwrap(),
        )
        .await;
        let id = created["id"].as_str().unwrap();

        let res = app
            .clone()
            .oneshot(put(
                "/api/schemas/물건",
                json!({
                    "type": "물건",
                    "category": "컬렉션",
                    "fields": {
                        "명칭": { "kind": "text", "required": true },
                        "상태": { "kind": "enum", "options": ["위시", "주문됨", "보유", "과거"] },
                        "가격": { "kind": "money" }
                    },
                    "renames": { "이름": "명칭" }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let body = body_json(
            app.oneshot(
                Request::builder()
                    .uri(format!("/api/entities/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
        )
        .await;
        assert_eq!(body["entity"]["data"]["명칭"], "잡화");
        assert!(body["entity"]["data"].get("이름").is_none());
    }

    #[tokio::test]
    async fn update는_기존_필드로_rename하는_요청을_거부한다() {
        let (state, _dir) = test_state().await;
        let app = build_app(state);

        let res = app
            .oneshot(put(
                "/api/schemas/물건",
                json!({
                    "type": "물건",
                    "category": "컬렉션",
                    "fields": {
                        "상태": { "kind": "text", "required": true },
                        "가격": { "kind": "money" }
                    },
                    "renames": { "이름": "상태" }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let body = body_json(res).await;
        assert_eq!(body["error"]["code"], "schema_validation");
        let message = body["error"]["message"].as_str().unwrap();
        assert!(message.contains("이름"));
        assert!(message.contains("상태"));
    }

    #[tokio::test]
    async fn update는_type_이름_변경을_거부한다() {
        let (state, _dir) = test_state().await;
        let app = build_app(state);

        let res = app
            .oneshot(put(
                "/api/schemas/물건",
                json!({
                    "type": "다른이름",
                    "category": "컬렉션",
                    "fields": { "이름": { "kind": "text", "required": true } }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let body = body_json(res).await;
        assert_eq!(body["error"]["code"], "schema_immutable");
    }

    #[tokio::test]
    async fn update는_extends_변경을_거부한다() {
        let (state, _dir) = test_state().await;
        let app = build_app(state);

        let res = app
            .oneshot(put(
                "/api/schemas/시계",
                json!({
                    "type": "시계",
                    "fields": { "무브먼트": { "kind": "text" } }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let body = body_json(res).await;
        assert_eq!(body["error"]["code"], "schema_immutable");
    }

    #[tokio::test]
    async fn 빈_미참조_타입은_삭제된다() {
        let (state, _dir) = test_state().await;
        let sdir = state.schemas_dir.clone();
        let app = build_app(state);

        let res = app
            .clone()
            .oneshot(post(
                "/api/schemas",
                json!({
                    "type": "북마크",
                    "fields": { "제목": { "kind": "text", "required": true } }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);
        assert!(sdir.join("북마크.yaml").exists());

        let res = app
            .clone()
            .oneshot(del("/api/schemas/북마크"))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        assert!(!sdir.join("북마크.yaml").exists());

        let body = body_json(
            app.oneshot(
                Request::builder()
                    .uri("/api/schemas")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
        )
        .await;
        assert!(body["types"].get("북마크").is_none());
    }

    #[tokio::test]
    async fn 엔티티_있는_타입_삭제는_409() {
        let (state, _dir) = test_state().await;
        let app = build_app(state);

        let res = app
            .clone()
            .oneshot(post(
                "/api/entities",
                json!({
                    "type": "물건",
                    "data": { "이름": "잡화" }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);

        let res = app.oneshot(del("/api/schemas/물건")).await.unwrap();
        assert_eq!(res.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn 자식이_extends하는_타입_삭제는_409() {
        let (state, _dir) = test_state().await;
        let app = build_app(state);

        let res = app.oneshot(del("/api/schemas/물건")).await.unwrap();
        assert_eq!(res.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn ref_target으로_참조되는_타입_삭제는_409() {
        let (state, _dir) = test_state().await;
        let app = build_app(state);

        let res = app
            .clone()
            .oneshot(post(
                "/api/schemas",
                json!({
                    "type": "장소",
                    "fields": { "이름": { "kind": "text", "required": true } }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);

        let res = app
            .clone()
            .oneshot(post(
                "/api/schemas",
                json!({
                    "type": "방문",
                    "fields": { "곳": { "kind": "ref", "target": "장소" } }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);

        let res = app.oneshot(del("/api/schemas/장소")).await.unwrap();
        assert_eq!(res.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn non_ref_target_메타데이터는_타입_삭제를_막지_않는다() {
        let (state, _dir) = test_state().await;
        let app = build_app(state);

        let res = app
            .clone()
            .oneshot(post(
                "/api/schemas",
                json!({
                    "type": "장소",
                    "fields": { "이름": { "kind": "text", "required": true } }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);

        let res = app
            .clone()
            .oneshot(post(
                "/api/schemas",
                json!({
                    "type": "방문메모",
                    "fields": { "장소명": { "kind": "text", "target": "장소" } }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);

        let res = app.clone().oneshot(del("/api/schemas/장소")).await.unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);

        let body = body_json(
            app.oneshot(
                Request::builder()
                    .uri("/api/schemas")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
        )
        .await;
        assert!(body["types"].get("장소").is_none());
        assert!(body["types"].get("방문메모").is_some());
    }

    #[tokio::test]
    async fn list_ref_target으로_참조되는_타입_삭제는_409() {
        let (state, _dir) = test_state().await;
        let app = build_app(state);

        let res = app
            .clone()
            .oneshot(post(
                "/api/schemas",
                json!({
                    "type": "장소",
                    "fields": { "이름": { "kind": "text", "required": true } }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);

        let res = app
            .clone()
            .oneshot(post(
                "/api/schemas",
                json!({
                    "type": "여행",
                    "fields": { "장소들": { "kind": "list<ref>", "target": "장소" } }
                }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);

        let res = app.oneshot(del("/api/schemas/장소")).await.unwrap();
        assert_eq!(res.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn 없는_타입_삭제는_404() {
        let (state, _dir) = test_state().await;
        let app = build_app(state);

        let res = app.oneshot(del("/api/schemas/없는타입")).await.unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn 원시_스키마는_own필드와_상속필드를_분리해_준다() {
        let (state, _dir) = test_state().await;
        let app = build_app(state);

        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/schemas/시계")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = body_json(res).await;

        assert_eq!(body["type"], "시계");
        assert_eq!(body["extends"], "물건");
        assert!(body["fields"].get("무브먼트").is_some());
        assert!(body["fields"].get("이름").is_none());
        assert!(body["inherited"].get("이름").is_some());
        assert!(body["inherited"].get("상태").is_some());
        assert!(body["inherited"].get("가격").is_some());
        assert!(body["inherited"].get("무브먼트").is_none());
    }

    #[tokio::test]
    async fn 없는_타입_원시조회는_404() {
        let (state, _dir) = test_state().await;
        let app = build_app(state);

        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/schemas/없는타입")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }
}
