use crate::entity::validate::ValidationError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("알 수 없는 타입 '{0}'")]
    UnknownType(String),
    #[error(transparent)]
    Validation(#[from] ValidationError),
    #[error("엔티티를 찾을 수 없음: {0}")]
    NotFound(String),
    #[error("삭제 불가: {}곳에서 참조 중", referrers.len())]
    DeleteBlocked { referrers: Vec<crate::entity::store::RefEdge> },
    #[error("DB 오류: {0}")]
    Db(#[from] sqlx::Error),
}

#[derive(Debug, Error)]
pub enum SchemaError {
    #[error("{file}: YAML 파싱 실패: {source}")]
    Parse {
        file: String,
        #[source]
        source: serde_yaml::Error,
    },
    #[error("{file}: 타입 '{name}' 중복 정의 (이미 {first}에 정의됨)")]
    DuplicateType { file: String, name: String, first: String },
    #[error("스키마 디렉터리 읽기 실패: {0}")]
    Io(#[from] std::io::Error),
    #[error("타입 '{ty}': 부모 '{parent}'를 찾을 수 없음")]
    UnknownParent { ty: String, parent: String },
    #[error("순환 상속 감지: {chain}")]
    Cycle { chain: String },
    #[error("타입 '{ty}' 필드 '{field}': 지원하지 않는 kind '{value}'")]
    BadKind { ty: String, field: String, value: String },
    #[error("타입 '{ty}' 필드 '{field}': enum은 options가 필요함")]
    EnumWithoutOptions { ty: String, field: String },
    #[error("타입 '{ty}' 필드 '{field}': ref는 target이 필요함")]
    RefWithoutTarget { ty: String, field: String },
    #[error("타입 '{ty}': field_order에 존재하지 않는 필드 '{field}'")]
    UnknownFieldInOrder { ty: String, field: String },
}
