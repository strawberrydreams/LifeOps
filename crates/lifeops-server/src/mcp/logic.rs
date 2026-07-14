use crate::mcp::convert::core_error_text;
use crate::mcp::LifeOpsMcp;
use lifeops_core::error::CoreError;
use serde_json::{json, Value};

impl LifeOpsMcp {
    /// 전체 타입의 이름·분류·상속·필드 요약을 반환한다.
    pub async fn do_list_types(&self) -> Result<Value, String> {
        let schemas = self.state.schemas.read().await;
        let types = schemas
            .names()
            .into_iter()
            .filter_map(|name| schemas.get(name))
            .map(|schema| {
                let fields = schema
                    .fields
                    .iter()
                    .map(|(name, field)| {
                        json!({ "name": name, "kind": field.kind, "required": field.required })
                    })
                    .collect::<Vec<_>>();
                json!({
                    "name": schema.name,
                    "category": schema.category,
                    "singleton": schema.singleton,
                    "extends": schema.extends,
                    "fields": fields,
                })
            })
            .collect::<Vec<_>>();
        Ok(json!({ "types": types }))
    }

    /// 한 타입의 상속 병합된 스키마를 반환한다.
    pub async fn do_get_schema(&self, type_name: &str) -> Result<Value, String> {
        let schemas = self.state.schemas.read().await;
        schemas
            .get(type_name)
            .map(|schema| json!(schema))
            .ok_or_else(|| core_error_text(&CoreError::UnknownType(type_name.to_string())))
    }
}

#[cfg(test)]
mod tests {
    use crate::mcp::LifeOpsMcp;
    use crate::state::test_state;

    #[tokio::test]
    async fn list_types는_시드타입들을_요약한다() {
        let (state, _dir) = test_state().await;
        let mcp = LifeOpsMcp::new(state);
        let value = mcp.do_list_types().await.unwrap();
        let types = value["types"].as_array().unwrap();
        let names = types
            .iter()
            .map(|entity_type| entity_type["name"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert!(names.contains(&"물건"));
        assert!(names.contains(&"시계"));
        assert!(names.contains(&"프로필"));
        let watch = types.iter().find(|entity_type| entity_type["name"] == "시계").unwrap();
        assert_eq!(watch["extends"], "물건");
        assert!(watch["fields"]
            .as_array()
            .unwrap()
            .iter()
            .any(|field| field["name"] == "이름"));
    }

    #[tokio::test]
    async fn get_schema는_해석된_필드를_준다() {
        let (state, _dir) = test_state().await;
        let mcp = LifeOpsMcp::new(state);
        let value = mcp.do_get_schema("시계").await.unwrap();
        assert_eq!(value["name"], "시계");
        assert!(value["fields"].get("가격").is_some());
    }

    #[tokio::test]
    async fn get_schema_없는타입은_에러문장() {
        let (state, _dir) = test_state().await;
        let mcp = LifeOpsMcp::new(state);
        let error = mcp.do_get_schema("유령타입").await.unwrap_err();
        assert!(error.contains("유령타입"));
    }
}
