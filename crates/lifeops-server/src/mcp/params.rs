use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map, Value};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetSchemaParams {
    /// 조회할 타입 이름
    #[serde(rename = "type")]
    pub type_name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct QueryParams {
    /// 조회할 타입(상속 자식 포함)
    #[serde(rename = "type")]
    pub type_name: String,
    /// 필드별 조건. 스칼라는 eq, 객체는 lt/gt/lte/gte/month 연산자를 뜻한다.
    #[serde(default)]
    pub filter: Option<Map<String, Value>>,
    /// 정렬 필드. 앞에 '-'를 붙이면 내림차순이다.
    #[serde(default)]
    pub sort: Option<String>,
    /// 최대 반환 개수
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetEntityParams {
    /// 엔티티 id
    pub id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateParams {
    /// 생성할 타입 이름
    #[serde(rename = "type")]
    pub type_name: String,
    /// 필드→값 맵. 스키마 검증을 통과해야 저장된다.
    pub data: Map<String, Value>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateParams {
    /// 수정할 엔티티 id
    pub id: String,
    /// 바꿀 필드만 담은 PATCH 맵
    pub patch: Map<String, Value>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DeleteParams {
    /// 삭제할 엔티티 id
    pub id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RunPageParams {
    /// 실행할 페이지(뷰) 이름
    pub name: String,
}
