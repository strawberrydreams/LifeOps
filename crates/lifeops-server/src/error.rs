use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use lifeops_core::error::{CoreError, ViewError};
use serde_json::json;

#[allow(dead_code)]
pub struct ApiError(pub StatusCode, pub serde_json::Value);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(self.1)).into_response()
    }
}

impl From<CoreError> for ApiError {
    fn from(e: CoreError) -> Self {
        match e {
            CoreError::Validation(v) => {
                let fields: Vec<_> =
                    v.0.iter()
                        .map(|f| json!({ "field": f.field, "message": f.message }))
                        .collect();
                ApiError(
                    StatusCode::BAD_REQUEST,
                    json!({ "error": { "code": "validation", "message": "검증 실패", "fields": fields } }),
                )
            }
            CoreError::UnknownType(t) => ApiError(
                StatusCode::NOT_FOUND,
                json!({ "error": { "code": "unknown_type", "message": format!("알 수 없는 타입 '{t}'") } }),
            ),
            CoreError::NotFound(id) => ApiError(
                StatusCode::NOT_FOUND,
                json!({ "error": { "code": "not_found", "message": format!("엔티티를 찾을 수 없음: {id}") } }),
            ),
            CoreError::DeleteBlocked { referrers } => {
                let refs: Vec<_> = referrers
                    .iter()
                    .map(|r| {
                        json!({
                            "from_id": r.from_id,
                            "from_type": r.from_type,
                            "field_name": r.field_name
                        })
                    })
                    .collect();
                ApiError(
                    StatusCode::CONFLICT,
                    json!({ "error": { "code": "delete_blocked", "message": format!("{}곳에서 참조 중", refs.len()), "referrers": refs } }),
                )
            }
            CoreError::Db(e) => {
                tracing::error!("DB 오류: {e}");
                ApiError(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    json!({ "error": { "code": "db", "message": "내부 서버 오류" } }),
                )
            }
        }
    }
}

impl From<ViewError> for ApiError {
    fn from(e: ViewError) -> Self {
        match e {
            ViewError::Core(c) => c.into(),
            ViewError::UnknownSource(_)
            | ViewError::UnknownField { .. }
            | ViewError::BadAggregate { .. }
            | ViewError::CurrencyMismatch { .. }
            | ViewError::BadDateToken { .. }
            | ViewError::MissingChartAxis { .. }
            | ViewError::DuplicatePage { .. } => ApiError(
                StatusCode::BAD_REQUEST,
                json!({ "error": { "code": "view", "message": e.to_string() } }),
            ),
            ViewError::Io(_) | ViewError::Parse { .. } => {
                tracing::error!("뷰 로드 오류: {e}");
                ApiError(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    json!({ "error": { "code": "view_load", "message": e.to_string() } }),
                )
            }
        }
    }
}
