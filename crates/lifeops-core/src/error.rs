use thiserror::Error;

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
}
