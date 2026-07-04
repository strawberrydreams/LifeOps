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
    DeleteBlocked {
        referrers: Vec<crate::entity::store::RefEdge>,
    },
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
    DuplicateType {
        file: String,
        name: String,
        first: String,
    },
    #[error("스키마 디렉터리 읽기 실패: {0}")]
    Io(#[from] std::io::Error),
    #[error("타입 '{ty}': 부모 '{parent}'를 찾을 수 없음")]
    UnknownParent { ty: String, parent: String },
    #[error("순환 상속 감지: {chain}")]
    Cycle { chain: String },
    #[error("타입 '{ty}' 필드 '{field}': 지원하지 않는 kind '{value}'")]
    BadKind {
        ty: String,
        field: String,
        value: String,
    },
    #[error("타입 '{ty}' 필드 '{field}': enum은 options가 필요함")]
    EnumWithoutOptions { ty: String, field: String },
    #[error("타입 '{ty}': field_order에 존재하지 않는 필드 '{field}'")]
    UnknownFieldInOrder { ty: String, field: String },
    #[error("타입 '{ty}' behaviors 오류: {message}")]
    BadBehavior { ty: String, message: String },
}

#[derive(Debug)]
pub enum ViewError {
    UnknownSource(String),
    UnknownField {
        view: String,
        source: String,
        field: String,
    },
    BadAggregate {
        view: String,
        expr: String,
    },
    CurrencyMismatch {
        view: String,
        field: String,
    },
    BadDateToken {
        view: String,
        field: String,
        token: String,
    },
    Io(std::io::Error),
    Parse {
        file: String,
        source: serde_yaml::Error,
    },
    DuplicatePage {
        file: String,
        page: String,
        first: String,
    },
    Core(crate::error::CoreError),
}

impl std::fmt::Display for ViewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ViewError::UnknownSource(source) => write!(f, "알 수 없는 source 타입 '{source}'"),
            ViewError::UnknownField {
                view,
                source,
                field,
            } => write!(f, "뷰 '{view}': source '{source}'에 없는 필드 '{field}'"),
            ViewError::BadAggregate { view, expr } => {
                write!(f, "뷰 '{view}': 잘못된 집계식 '{expr}' (형식: 함수(필드))")
            }
            ViewError::CurrencyMismatch { view, field } => {
                write!(
                    f,
                    "뷰 '{view}' 필드 '{field}': 통화가 섞여 합계를 낼 수 없음"
                )
            }
            ViewError::BadDateToken { view, field, token } => write!(
                f,
                "뷰 '{view}' 필드 '{field}': 날짜 토큰 '{token}'은 date 필드에서만, $today[±Nd] 형식만 지원"
            ),
            ViewError::Io(source) => write!(f, "페이지 디렉터리 로드 실패: {source}"),
            ViewError::Parse { file, source } => {
                write!(f, "{file}: 페이지 YAML 파싱 실패: {source}")
            }
            ViewError::DuplicatePage { file, page, first } => {
                write!(
                    f,
                    "{file}: 페이지 '{page}' 중복 정의 (이미 {first}에 정의됨)"
                )
            }
            ViewError::Core(source) => source.fmt(f),
        }
    }
}

impl std::error::Error for ViewError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ViewError::Io(source) => Some(source),
            ViewError::Parse { source, .. } => Some(source),
            ViewError::Core(source) => Some(source),
            ViewError::UnknownSource(_)
            | ViewError::UnknownField { .. }
            | ViewError::BadAggregate { .. }
            | ViewError::CurrencyMismatch { .. }
            | ViewError::BadDateToken { .. }
            | ViewError::DuplicatePage { .. } => None,
        }
    }
}

impl From<std::io::Error> for ViewError {
    fn from(source: std::io::Error) -> Self {
        ViewError::Io(source)
    }
}

impl From<crate::error::CoreError> for ViewError {
    fn from(source: crate::error::CoreError) -> Self {
        ViewError::Core(source)
    }
}
