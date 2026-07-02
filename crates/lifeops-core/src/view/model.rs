use crate::entity::Entity;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// 필드명 -> 조건. 스칼라 값은 eq, `{month|lt|gt: v}` 맵은 연산자.
pub type Filter = IndexMap<String, serde_json::Value>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Layout {
    #[default]
    Table,
    Checklist,
    Card,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ViewBlock {
    pub view: String,
    pub source: String,
    #[serde(default)]
    pub filter: Option<Filter>,
    #[serde(default)]
    pub sort: Option<String>,
    #[serde(default)]
    pub layout: Layout,
    #[serde(default)]
    pub columns: Option<Vec<String>>,
    /// 집계명 -> "함수(필드)" 문자열 (예: "sum(가격)")
    #[serde(default)]
    pub aggregate: Option<IndexMap<String, String>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PageDef {
    pub page: String,
    pub blocks: Vec<ViewBlock>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ViewResult {
    pub view: String,
    pub layout: Layout,
    pub columns: Option<Vec<String>>,
    pub entities: Vec<Entity>,
    pub aggregates: IndexMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PageResult {
    pub page: String,
    pub blocks: Vec<ViewResult>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 뷰블록_yaml_파싱_순서와_기본값() {
        let yaml = "view: 다가오는 택배\nsource: 물건\nfilter: { 상태: 주문됨 }\nsort: 배송예정일\nlayout: table\ncolumns: [이름, 배송예정일, 가격]\n";
        let block: ViewBlock = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(block.view, "다가오는 택배");
        assert_eq!(block.source, "물건");
        assert_eq!(block.layout, Layout::Table);
        assert_eq!(
            block.columns.as_deref().unwrap(),
            ["이름", "배송예정일", "가격"]
        );
        assert_eq!(block.filter.unwrap()["상태"], serde_json::json!("주문됨"));
    }

    #[test]
    fn layout_기본값은_table이고_소문자() {
        let block: ViewBlock = serde_yaml::from_str("view: v\nsource: 할일\n").unwrap();
        assert_eq!(block.layout, Layout::Table);
        let cl: ViewBlock =
            serde_yaml::from_str("view: v\nsource: 할일\nlayout: checklist\n").unwrap();
        assert_eq!(cl.layout, Layout::Checklist);
    }

    #[test]
    fn 페이지_여러_블록_파싱() {
        let yaml = "page: 데일리 대시보드\nblocks:\n  - view: 우선순위\n    source: 할일\n    filter: { 완료: false }\n    layout: checklist\n  - view: 이번 달 지출\n    source: 물건\n    filter: { 상태: 주문됨 }\n    aggregate: { 합계: sum(가격) }\n";
        let page: PageDef = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(page.page, "데일리 대시보드");
        assert_eq!(page.blocks.len(), 2);
        assert_eq!(
            page.blocks[1].aggregate.as_ref().unwrap()["합계"],
            "sum(가격)"
        );
    }
}
