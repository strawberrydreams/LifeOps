use crate::mcp::convert::{core_error_text, view_error_text};
use crate::mcp::params::QueryParams;
use crate::mcp::LifeOpsMcp;
use lifeops_core::entity::recurrence::{apply_recurrence, RecurrenceOutcome};
use lifeops_core::error::CoreError;
use lifeops_core::view::{run_page, run_view, ChartType, Layout, ViewBlock};
use serde_json::{json, Map, Value};

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

    /// 뷰 엔진으로 타입·필터·정렬·limit 조회를 수행한다.
    pub async fn do_query_entities(&self, params: QueryParams) -> Result<Value, String> {
        let schemas = self.state.schemas.read().await;
        let block = ViewBlock {
            view: "mcp".to_string(),
            source: params.type_name,
            filter: params.filter.map(|filter| filter.into_iter().collect()),
            sort: params.sort,
            layout: Layout::Table,
            columns: None,
            aggregate: None,
            limit: params.limit,
            x: None,
            y: None,
            series: None,
            chart_type: ChartType::Line,
            sections: None,
        };
        let result = run_view(&self.state.store, &schemas, &block)
            .await
            .map_err(|error| view_error_text(&error))?;
        Ok(json!({ "entities": result.entities }))
    }

    /// id로 엔티티 한 건과 그 엔티티를 가리키는 역링크를 반환한다.
    pub async fn do_get_entity(&self, id: &str) -> Result<Value, String> {
        let entity = self
            .state
            .store
            .get(id)
            .await
            .map_err(|error| core_error_text(&error))?
            .ok_or_else(|| core_error_text(&CoreError::NotFound(id.to_string())))?;
        let backlinks = self
            .state
            .store
            .backlinks(id)
            .await
            .map_err(|error| core_error_text(&error))?;
        Ok(json!({ "entity": entity, "backlinks": backlinks }))
    }

    /// 스키마 검증을 거쳐 엔티티를 생성한다.
    pub async fn do_create_entity(
        &self,
        type_name: &str,
        data: Map<String, Value>,
    ) -> Result<Value, String> {
        let schemas = self.state.schemas.read().await;
        let entity = self
            .state
            .store
            .create(&schemas, type_name, data)
            .await
            .map_err(|error| core_error_text(&error))?;
        Ok(json!(entity))
    }

    /// 부분 수정 후 반복 behavior가 적용되면 다음 회차를 함께 반환한다.
    pub async fn do_update_entity(
        &self,
        id: &str,
        patch: Map<String, Value>,
    ) -> Result<Value, String> {
        let schemas = self.state.schemas.read().await;
        let before = self
            .state
            .store
            .get(id)
            .await
            .map_err(|error| core_error_text(&error))?
            .ok_or_else(|| core_error_text(&CoreError::NotFound(id.to_string())))?;
        let entity = self
            .state
            .store
            .update(&schemas, id, patch)
            .await
            .map_err(|error| core_error_text(&error))?;
        let outcome = apply_recurrence(
            &self.state.store,
            &schemas,
            &before,
            &entity,
            chrono::Local::now().date_naive(),
        )
        .await
        .map_err(|error| core_error_text(&error))?;

        let mut body = json!(entity);
        match outcome {
            RecurrenceOutcome::Spawned(spawned) => body["spawned"] = json!(spawned),
            RecurrenceOutcome::BadRule(rule) => {
                body["recurrence_warning"] = json!(format!("반복 규칙을 해석할 수 없음: {rule}"));
            }
            RecurrenceOutcome::NotApplicable => {}
        }
        Ok(body)
    }

    /// 참조 보호를 포함한 코어 삭제 경로로 엔티티를 삭제한다.
    pub async fn do_delete_entity(&self, id: &str) -> Result<Value, String> {
        self.state
            .store
            .delete(id)
            .await
            .map_err(|error| core_error_text(&error))?;
        Ok(json!({ "deleted": id }))
    }

    /// 저장된 페이지 정의 목록을 반환한다.
    pub async fn do_list_pages(&self) -> Result<Value, String> {
        let pages = self.state.pages.read().await;
        Ok(json!({ "pages": pages.all() }))
    }

    /// 저장된 페이지를 실행해 블록별 엔티티와 집계를 반환한다.
    pub async fn do_run_page(&self, name: &str) -> Result<Value, String> {
        let definition = {
            let pages = self.state.pages.read().await;
            pages
                .get(name)
                .cloned()
                .ok_or_else(|| format!("페이지 없음: {name}. list_pages로 확인하세요."))?
        };
        let schemas = self.state.schemas.read().await;
        let result = run_page(&self.state.store, &schemas, &definition)
            .await
            .map_err(|error| view_error_text(&error))?;
        Ok(json!(result))
    }
}

#[cfg(test)]
mod tests {
    use crate::mcp::params::QueryParams;
    use crate::mcp::LifeOpsMcp;
    use crate::state::test_state;
    use serde_json::json;

    async fn seed_watch(mcp: &LifeOpsMcp, name: &str, price: f64) -> String {
        let value = mcp
            .do_create_entity(
                "시계",
                json!({
                    "이름": name,
                    "가격": { "amount": price, "currency": "KRW" }
                })
                .as_object()
                .unwrap()
                .clone(),
            )
            .await
            .unwrap();
        value["id"].as_str().unwrap().to_string()
    }

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
        let watch = types
            .iter()
            .find(|entity_type| entity_type["name"] == "시계")
            .unwrap();
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

    #[tokio::test]
    async fn query_entities_필터와_정렬과_limit() {
        let (state, _dir) = test_state().await;
        let mcp = LifeOpsMcp::new(state);
        seed_watch(&mcp, "A", 300_000.0).await;
        seed_watch(&mcp, "B", 100_000.0).await;
        seed_watch(&mcp, "C", 650_000.0).await;

        let params = QueryParams {
            type_name: "물건".into(),
            filter: None,
            sort: Some("-가격".into()),
            limit: Some(2),
        };
        let value = mcp.do_query_entities(params).await.unwrap();
        let entities = value["entities"].as_array().unwrap();
        assert_eq!(entities.len(), 2);
        assert_eq!(entities[0]["data"]["이름"], "C");
        assert_eq!(entities[1]["data"]["이름"], "A");
    }

    #[tokio::test]
    async fn query_entities_없는필터필드는_에러문장() {
        let (state, _dir) = test_state().await;
        let mcp = LifeOpsMcp::new(state);
        let mut filter = serde_json::Map::new();
        filter.insert("유령".into(), json!("x"));
        let error = mcp
            .do_query_entities(QueryParams {
                type_name: "물건".into(),
                filter: Some(filter),
                sort: None,
                limit: None,
            })
            .await
            .unwrap_err();
        assert!(error.contains("유령"));
    }

    #[tokio::test]
    async fn query_entities_필터를_뷰엔진에_전달한다() {
        let (state, _dir) = test_state().await;
        let mcp = LifeOpsMcp::new(state);
        seed_watch(&mcp, "A", 300_000.0).await;
        seed_watch(&mcp, "B", 100_000.0).await;
        let mut filter = serde_json::Map::new();
        filter.insert("이름".into(), json!("B"));

        let value = mcp
            .do_query_entities(QueryParams {
                type_name: "물건".into(),
                filter: Some(filter),
                sort: None,
                limit: None,
            })
            .await
            .unwrap();
        let entities = value["entities"].as_array().unwrap();
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0]["data"]["이름"], "B");
    }

    #[tokio::test]
    async fn get_entity_역링크_포함() {
        let (state, _dir) = test_state().await;
        let mcp = LifeOpsMcp::new(state);
        let watch_id = seed_watch(&mcp, "미쿠", 200_000.0).await;
        mcp.do_create_entity(
            "할일",
            json!({ "내용": "개봉", "관련물건": [watch_id.clone()] })
                .as_object()
                .unwrap()
                .clone(),
        )
        .await
        .unwrap();
        let value = mcp.do_get_entity(&watch_id).await.unwrap();
        assert_eq!(value["entity"]["data"]["이름"], "미쿠");
        assert_eq!(value["backlinks"].as_array().unwrap().len(), 1);
        assert_eq!(value["backlinks"][0]["from_type"], "할일");
    }

    #[tokio::test]
    async fn get_entity_없는id는_에러문장() {
        let (state, _dir) = test_state().await;
        let mcp = LifeOpsMcp::new(state);
        assert!(mcp
            .do_get_entity("없는id")
            .await
            .unwrap_err()
            .contains("없는id"));
    }

    #[tokio::test]
    async fn create_검증실패는_필드에러_문장() {
        let (state, _dir) = test_state().await;
        let mcp = LifeOpsMcp::new(state);
        let error = mcp
            .do_create_entity(
                "시계",
                json!({ "상태": "위시" }).as_object().unwrap().clone(),
            )
            .await
            .unwrap_err();
        assert!(error.contains("이름"));
        assert!(error.contains("검증"));
    }

    #[tokio::test]
    async fn update_반복할일_완료시_spawned() {
        let (state, _dir) = test_state().await;
        let mcp = LifeOpsMcp::new(state);
        let created = mcp
            .do_create_entity(
                "할일",
                json!({ "내용": "청소", "완료": false, "마감일": "2026-01-05", "반복": "매주" })
                    .as_object()
                    .unwrap()
                    .clone(),
            )
            .await
            .unwrap();
        let id = created["id"].as_str().unwrap();
        let value = mcp
            .do_update_entity(id, json!({ "완료": true }).as_object().unwrap().clone())
            .await
            .unwrap();
        assert_eq!(value["data"]["완료"], json!(true));
        assert_eq!(value["spawned"]["data"]["완료"], json!(false));
        assert!(value["spawned"]["data"]["마감일"].as_str().unwrap() > "2026-01-05");
    }

    #[tokio::test]
    async fn delete_참조존재시_에러문장() {
        let (state, _dir) = test_state().await;
        let mcp = LifeOpsMcp::new(state);
        let watch_id = seed_watch(&mcp, "미쿠", 0.0).await;
        mcp.do_create_entity(
            "할일",
            json!({ "내용": "개봉", "관련물건": [watch_id.clone()] })
                .as_object()
                .unwrap()
                .clone(),
        )
        .await
        .unwrap();
        let error = mcp.do_delete_entity(&watch_id).await.unwrap_err();
        assert!(error.contains("삭제 불가"));
        assert!(error.contains("할일"));
    }

    #[tokio::test]
    async fn delete_성공은_id를_돌려준다() {
        let (state, _dir) = test_state().await;
        let mcp = LifeOpsMcp::new(state);
        let value = mcp
            .do_create_entity(
                "시계",
                json!({ "이름": "카시오" }).as_object().unwrap().clone(),
            )
            .await
            .unwrap();
        let id = value["id"].as_str().unwrap().to_string();
        let output = mcp.do_delete_entity(&id).await.unwrap();
        assert_eq!(output["deleted"], id);
    }

    #[tokio::test]
    async fn list_pages는_시드_프로필페이지를_포함() {
        let (state, _dir) = test_state().await;
        let mcp = LifeOpsMcp::new(state);
        let value = mcp.do_list_pages().await.unwrap();
        let pages = value["pages"].as_array().unwrap();
        assert!(pages.iter().any(|page| page["page"] == "프로필"));
    }

    #[tokio::test]
    async fn run_page_프로필은_엔티티와_블록을_준다() {
        let (state, _dir) = test_state().await;
        let mcp = LifeOpsMcp::new(state);
        mcp.do_create_entity(
            "프로필",
            json!({ "이름": "미쿠", "거주지": "삿포로" })
                .as_object()
                .unwrap()
                .clone(),
        )
        .await
        .unwrap();
        let value = mcp.do_run_page("프로필").await.unwrap();
        assert_eq!(value["page"], "프로필");
        assert_eq!(value["blocks"][0]["entities"][0]["data"]["이름"], "미쿠");
    }

    #[tokio::test]
    async fn run_page_없는페이지는_에러문장() {
        let (state, _dir) = test_state().await;
        let mcp = LifeOpsMcp::new(state);
        assert!(mcp
            .do_run_page("없는페이지")
            .await
            .unwrap_err()
            .contains("없는페이지"));
    }
}
