use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetSchemaParams {
    /// 조회할 타입 이름
    #[serde(rename = "type")]
    pub type_name: String,
}
