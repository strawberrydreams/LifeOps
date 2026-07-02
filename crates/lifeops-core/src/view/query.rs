use crate::entity::{Entity, EntityStore};
use crate::error::ViewError;
use crate::schema::{FieldKind, ResolvedSchema, SchemaSet};
use crate::view::model::{Filter, ViewBlock, ViewResult};
use indexmap::IndexMap;
use serde_json::Value;
use std::cmp::Ordering;

pub async fn run_view(
    store: &EntityStore,
    schemas: &SchemaSet,
    block: &ViewBlock,
) -> Result<ViewResult, ViewError> {
    let schema = schemas
        .get(&block.source)
        .ok_or_else(|| ViewError::UnknownSource(block.source.clone()))?;

    validate_filter(block, schema)?;
    validate_sort(block, schema)?;

    let mut entities = store.list(&schemas.family_of(&block.source)).await?;
    if let Some(filter) = &block.filter {
        entities.retain(|entity| matches(entity, schema, filter));
    }
    if let Some(sort) = &block.sort {
        sort_entities(&mut entities, schema, sort);
    }

    Ok(ViewResult {
        view: block.view.clone(),
        layout: block.layout,
        columns: block.columns.clone(),
        entities,
        aggregates: IndexMap::new(),
    })
}

fn validate_filter(block: &ViewBlock, schema: &ResolvedSchema) -> Result<(), ViewError> {
    if let Some(filter) = &block.filter {
        for field in filter.keys() {
            if !schema.fields.contains_key(field) {
                return Err(unknown_field(block, field));
            }
        }
    }
    Ok(())
}

fn validate_sort(block: &ViewBlock, schema: &ResolvedSchema) -> Result<(), ViewError> {
    if let Some(sort) = &block.sort {
        let field = sort.strip_prefix('-').unwrap_or(sort);
        if !schema.fields.contains_key(field) {
            return Err(unknown_field(block, field));
        }
    }
    Ok(())
}

fn unknown_field(block: &ViewBlock, field: &str) -> ViewError {
    ViewError::UnknownField {
        view: block.view.clone(),
        source: block.source.clone(),
        field: field.to_string(),
    }
}

fn matches(entity: &Entity, schema: &ResolvedSchema, filter: &Filter) -> bool {
    filter.iter().all(|(field, condition)| {
        match_one(
            entity.data.get(field),
            &schema.fields[field].kind,
            condition,
        )
    })
}

fn match_one(actual: Option<&Value>, kind: &FieldKind, condition: &Value) -> bool {
    let Some(op_map) = condition.as_object() else {
        return scalar_eq(actual, condition);
    };
    if op_map.len() != 1 {
        return false;
    }

    let (op, arg) = op_map.iter().next().expect("operator map length checked");
    match op.as_str() {
        "month" => date_month_matches(actual, arg),
        "lt" => compare_filter(actual, kind, arg).is_some_and(|ord| ord == Ordering::Less),
        "gt" => compare_filter(actual, kind, arg).is_some_and(|ord| ord == Ordering::Greater),
        _ => false,
    }
}

fn scalar_eq(actual: Option<&Value>, condition: &Value) -> bool {
    let Some(actual) = actual else {
        return false;
    };
    match (actual.as_f64(), condition.as_f64()) {
        (Some(left), Some(right)) => left == right,
        _ => actual == condition,
    }
}

fn date_month_matches(actual: Option<&Value>, arg: &Value) -> bool {
    match (actual, arg.as_str()) {
        (Some(Value::String(d)), Some(m)) => d.starts_with(m),
        _ => false,
    }
}

fn compare_filter(actual: Option<&Value>, kind: &FieldKind, arg: &Value) -> Option<Ordering> {
    match kind {
        FieldKind::Date => {
            let left = actual?.as_str()?;
            let right = arg.as_str()?;
            Some(left.cmp(right))
        }
        _ => {
            let left = extract_f64(actual?)?;
            let right = extract_f64(arg)?;
            left.partial_cmp(&right)
        }
    }
}

fn extract_f64(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_object()?.get("amount")?.as_f64())
}

fn sort_entities(entities: &mut [Entity], schema: &ResolvedSchema, sort: &str) {
    let (descending, field) = match sort.strip_prefix('-') {
        Some(field) => (true, field),
        None => (false, sort),
    };
    let kind = &schema.fields[field].kind;

    entities.sort_by(|left, right| {
        compare_sort(
            left.data.get(field),
            right.data.get(field),
            kind,
            descending,
        )
    });
}

fn compare_sort(
    left: Option<&Value>,
    right: Option<&Value>,
    kind: &FieldKind,
    descending: bool,
) -> Ordering {
    let left_key = sort_key(left, kind);
    let right_key = sort_key(right, kind);
    let has_missing = matches!(
        (&left_key, &right_key),
        (SortKey::Missing, _) | (_, SortKey::Missing)
    );
    let ord = match (left_key, right_key) {
        (SortKey::Missing, SortKey::Missing) => Ordering::Equal,
        (SortKey::Missing, _) => Ordering::Greater,
        (_, SortKey::Missing) => Ordering::Less,
        (SortKey::Number(left), SortKey::Number(right)) => {
            left.partial_cmp(&right).unwrap_or(Ordering::Equal)
        }
        (SortKey::Text(left), SortKey::Text(right)) => left.cmp(&right),
        (left, right) => left.rank().cmp(&right.rank()),
    };
    if descending && !has_missing && ord != Ordering::Equal {
        ord.reverse()
    } else {
        ord
    }
}

fn sort_key(value: Option<&Value>, kind: &FieldKind) -> SortKey {
    let Some(value) = value else {
        return SortKey::Missing;
    };
    match kind {
        FieldKind::Number | FieldKind::Money => {
            extract_f64(value).map_or(SortKey::Missing, SortKey::Number)
        }
        FieldKind::Text | FieldKind::Enum | FieldKind::Date => value
            .as_str()
            .map(|s| SortKey::Text(s.to_string()))
            .unwrap_or(SortKey::Missing),
        _ => SortKey::Text(value.to_string()),
    }
}

enum SortKey {
    Missing,
    Number(f64),
    Text(String),
}

impl SortKey {
    fn rank(&self) -> u8 {
        match self {
            SortKey::Missing => 2,
            SortKey::Number(_) => 0,
            SortKey::Text(_) => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::EntityStore;
    use crate::schema::SchemaSet;
    use serde_json::{json, Map, Value};

    fn schemas() -> SchemaSet {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("물건.yaml"),
            "type: 물건\nfields:\n  이름: { kind: text, required: true }\n  상태: { kind: enum, options: [위시, 주문됨, 보유, 과거] }\n  가격: { kind: money }\n  배송예정일: { kind: date }\n",
        ).unwrap();
        std::fs::write(
            dir.path().join("시계.yaml"),
            "type: 시계\nextends: 물건\nfields:\n  무브먼트: { kind: text }\n",
        )
        .unwrap();
        SchemaSet::load_dir(dir.path()).unwrap()
    }
    fn obj(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    async fn seed(store: &EntityStore, s: &SchemaSet) {
        store.create(s, "물건", obj(json!({ "이름": "A", "상태": "주문됨", "가격": {"amount": 300000.0, "currency": "KRW"}, "배송예정일": "2026-07-10" }))).await.unwrap();
        store.create(s, "물건", obj(json!({ "이름": "B", "상태": "위시", "가격": {"amount": 100000.0, "currency": "KRW"}, "배송예정일": "2026-08-01" }))).await.unwrap();
        store.create(s, "시계", obj(json!({ "이름": "C", "상태": "주문됨", "가격": {"amount": 650000.0, "currency": "KRW"}, "배송예정일": "2026-07-20" }))).await.unwrap();
    }

    fn block(yaml: &str) -> crate::view::ViewBlock {
        serde_yaml::from_str(yaml).unwrap()
    }

    #[tokio::test]
    async fn eq_필터와_family_확장() {
        let s = schemas();
        let store = EntityStore::open_in_memory().await.unwrap();
        seed(&store, &s).await;
        let r = run_view(
            &store,
            &s,
            &block("view: v\nsource: 물건\nfilter: { 상태: 주문됨 }\n"),
        )
        .await
        .unwrap();
        let names: Vec<&str> = r
            .entities
            .iter()
            .map(|e| e.data["이름"].as_str().unwrap())
            .collect();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"A") && names.contains(&"C"));
    }

    #[tokio::test]
    async fn month_필터() {
        let s = schemas();
        let store = EntityStore::open_in_memory().await.unwrap();
        seed(&store, &s).await;
        let r = run_view(
            &store,
            &s,
            &block("view: v\nsource: 물건\nfilter: { 배송예정일: { month: 2026-07 } }\n"),
        )
        .await
        .unwrap();
        assert_eq!(r.entities.len(), 2);
    }

    #[tokio::test]
    async fn gt_필터_money와_정렬() {
        let s = schemas();
        let store = EntityStore::open_in_memory().await.unwrap();
        seed(&store, &s).await;
        let r = run_view(
            &store,
            &s,
            &block("view: v\nsource: 물건\nfilter: { 가격: { gt: 200000 } }\nsort: 가격\n"),
        )
        .await
        .unwrap();
        let names: Vec<&str> = r
            .entities
            .iter()
            .map(|e| e.data["이름"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["A", "C"]);
        let r2 = run_view(
            &store,
            &s,
            &block("view: v\nsource: 물건\nfilter: { 가격: { gt: 200000 } }\nsort: -가격\n"),
        )
        .await
        .unwrap();
        let names2: Vec<&str> = r2
            .entities
            .iter()
            .map(|e| e.data["이름"].as_str().unwrap())
            .collect();
        assert_eq!(names2, ["C", "A"]);
    }

    #[tokio::test]
    async fn null_정렬값은_오름차순과_내림차순_모두_마지막() {
        let s = schemas();
        let store = EntityStore::open_in_memory().await.unwrap();
        store
            .create(
                &s,
                "물건",
                obj(json!({ "이름": "저가", "가격": {"amount": 100000.0, "currency": "KRW"} })),
            )
            .await
            .unwrap();
        store
            .create(
                &s,
                "물건",
                obj(json!({ "이름": "고가", "가격": {"amount": 300000.0, "currency": "KRW"} })),
            )
            .await
            .unwrap();
        store
            .create(&s, "물건", obj(json!({ "이름": "가격없음", "가격": null })))
            .await
            .unwrap();

        let asc = run_view(&store, &s, &block("view: v\nsource: 물건\nsort: 가격\n"))
            .await
            .unwrap();
        let asc_names: Vec<&str> = asc
            .entities
            .iter()
            .map(|e| e.data["이름"].as_str().unwrap())
            .collect();
        assert_eq!(asc_names, ["저가", "고가", "가격없음"]);

        let desc = run_view(&store, &s, &block("view: v\nsource: 물건\nsort: -가격\n"))
            .await
            .unwrap();
        let desc_names: Vec<&str> = desc
            .entities
            .iter()
            .map(|e| e.data["이름"].as_str().unwrap())
            .collect();
        assert_eq!(desc_names, ["고가", "저가", "가격없음"]);
    }

    #[tokio::test]
    async fn 없는_필드_필터는_에러() {
        let s = schemas();
        let store = EntityStore::open_in_memory().await.unwrap();
        let err = run_view(
            &store,
            &s,
            &block("view: v\nsource: 물건\nfilter: { 유령: 1 }\n"),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("유령"));
    }

    #[tokio::test]
    async fn 없는_source는_에러() {
        let s = schemas();
        let store = EntityStore::open_in_memory().await.unwrap();
        let err = run_view(&store, &s, &block("view: v\nsource: 유령타입\n"))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("유령타입"));
    }
}
