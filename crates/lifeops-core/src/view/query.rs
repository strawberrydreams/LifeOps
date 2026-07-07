use crate::entity::{Entity, EntityStore};
use crate::error::ViewError;
use crate::schema::{FieldKind, ResolvedSchema, SchemaSet};
use crate::view::model::{ChartPoint, ChartSeries, Filter, Layout, ViewBlock, ViewResult};
use chrono::NaiveDate;
use indexmap::IndexMap;
use serde_json::Value;
use std::cmp::Ordering;

pub async fn run_view(
    store: &EntityStore,
    schemas: &SchemaSet,
    block: &ViewBlock,
) -> Result<ViewResult, ViewError> {
    run_view_at(store, schemas, block, chrono::Local::now().date_naive()).await
}

pub async fn run_view_at(
    store: &EntityStore,
    schemas: &SchemaSet,
    block: &ViewBlock,
    today: NaiveDate,
) -> Result<ViewResult, ViewError> {
    let schema = schemas
        .get(&block.source)
        .ok_or_else(|| ViewError::UnknownSource(block.source.clone()))?;

    validate_filter(block, schema, today)?;
    validate_sort(block, schema)?;
    validate_chart(block, schema)?;

    let mut entities = store.list(&schemas.family_of(&block.source)).await?;
    if let Some(filter) = &block.filter {
        entities.retain(|entity| matches(entity, schema, filter, today));
    }

    let mut aggregates = IndexMap::new();
    if let Some(aggregate) = &block.aggregate {
        for (name, expr) in aggregate {
            aggregates.insert(
                name.clone(),
                compute_aggregate(&block.view, expr, schema, &entities)?,
            );
        }
    }

    if let Some(sort) = &block.sort {
        sort_entities(&mut entities, schema, sort);
    }
    if let Some(limit) = block.limit {
        entities.truncate(limit);
    }
    let chart = if block.layout == Layout::Chart {
        Some(build_chart_series(block, &entities))
    } else {
        None
    };
    let chart_type = if block.layout == Layout::Chart {
        Some(block.chart_type)
    } else {
        None
    };

    Ok(ViewResult {
        view: block.view.clone(),
        source: block.source.clone(),
        filter: block.filter.clone(),
        sort: block.sort.clone(),
        layout: block.layout,
        columns: block.columns.clone(),
        entities,
        aggregates,
        chart_type,
        chart,
    })
}

/// `$today[±Nd]` → YYYY-MM-DD. 토큰이 아니거나 형식이 틀리면 None.
pub fn resolve_today_token(s: &str, today: NaiveDate) -> Option<String> {
    let rest = s.strip_prefix("$today")?;
    let days: i64 = if rest.is_empty() {
        0
    } else {
        rest.strip_suffix('d')?.parse().ok()? // "+7" / "-3"
    };
    Some(
        (today + chrono::Duration::days(days))
            .format("%Y-%m-%d")
            .to_string(),
    )
}

fn validate_filter(
    block: &ViewBlock,
    schema: &ResolvedSchema,
    today: NaiveDate,
) -> Result<(), ViewError> {
    if let Some(filter) = &block.filter {
        for (field, condition) in filter {
            if !schema.fields.contains_key(field) {
                return Err(unknown_field(block, field));
            }
            let kind = &schema.fields[field].kind;
            let args: Vec<&Value> = match condition.as_object() {
                Some(map) => map.values().collect(),
                None => vec![condition],
            };
            for arg in args {
                if let Some(s) = arg.as_str() {
                    if s.starts_with("$today")
                        && (!matches!(kind, FieldKind::Date)
                            || resolve_today_token(s, today).is_none())
                    {
                        return Err(ViewError::BadDateToken {
                            view: block.view.clone(),
                            field: field.clone(),
                            token: s.to_string(),
                        });
                    }
                }
            }
        }
    }
    Ok(())
}

fn validate_sort(block: &ViewBlock, schema: &ResolvedSchema) -> Result<(), ViewError> {
    if let Some(sort) = &block.sort {
        let field = sort.strip_prefix('-').unwrap_or(sort);
        if is_system_column(field) {
            return Ok(());
        }
        if !schema.fields.contains_key(field) {
            return Err(unknown_field(block, field));
        }
    }
    Ok(())
}

fn validate_chart(block: &ViewBlock, schema: &ResolvedSchema) -> Result<(), ViewError> {
    if block.layout != Layout::Chart {
        return Ok(());
    }

    let x = block
        .x
        .as_deref()
        .ok_or_else(|| ViewError::MissingChartAxis {
            view: block.view.clone(),
            axis: "x",
        })?;
    let y = block
        .y
        .as_deref()
        .ok_or_else(|| ViewError::MissingChartAxis {
            view: block.view.clone(),
            axis: "y",
        })?;

    if !is_system_column(x) && !schema.fields.contains_key(x) {
        return Err(unknown_field(block, x));
    }
    if !is_system_column(y) && !schema.fields.contains_key(y) {
        return Err(unknown_field(block, y));
    }
    if let Some(series) = &block.series {
        if !schema.fields.contains_key(series) {
            return Err(unknown_field(block, series));
        }
    }

    Ok(())
}

pub fn is_system_column(field: &str) -> bool {
    field == "updated_at" || field == "created_at"
}

fn unknown_field(block: &ViewBlock, field: &str) -> ViewError {
    ViewError::UnknownField {
        view: block.view.clone(),
        source: block.source.clone(),
        field: field.to_string(),
    }
}

fn matches(entity: &Entity, schema: &ResolvedSchema, filter: &Filter, today: NaiveDate) -> bool {
    filter.iter().all(|(field, condition)| {
        match_one(
            entity.data.get(field),
            &schema.fields[field].kind,
            condition,
            today,
        )
    })
}

fn match_one(
    actual: Option<&Value>,
    kind: &FieldKind,
    condition: &Value,
    today: NaiveDate,
) -> bool {
    let Some(op_map) = condition.as_object() else {
        return scalar_eq(actual, &normalize_arg(kind, condition, today));
    };
    if op_map.len() != 1 {
        return false;
    }

    let (op, arg) = op_map.iter().next().expect("operator map length checked");
    let arg = normalize_arg(kind, arg, today);
    match op.as_str() {
        "month" => date_month_matches(actual, &arg),
        "lt" => compare_filter(actual, kind, &arg).is_some_and(|ord| ord == Ordering::Less),
        "gt" => compare_filter(actual, kind, &arg).is_some_and(|ord| ord == Ordering::Greater),
        "lte" => compare_filter(actual, kind, &arg).is_some_and(|ord| ord != Ordering::Greater),
        "gte" => compare_filter(actual, kind, &arg).is_some_and(|ord| ord != Ordering::Less),
        _ => false,
    }
}

/// 서버 자동 뷰(GET /api/entities)가 뷰 엔진과 동일한 필터 의미론을 쓰기 위한 진입점.
pub fn matches_condition(
    actual: Option<&Value>,
    kind: &FieldKind,
    condition: &Value,
    today: NaiveDate,
) -> bool {
    match_one(actual, kind, condition, today)
}

/// date 필드의 `$today` 토큰을 실제 날짜 문자열로 치환. 그 외에는 원본 유지.
fn normalize_arg(kind: &FieldKind, arg: &Value, today: NaiveDate) -> Value {
    if matches!(kind, FieldKind::Date) {
        if let Some(s) = arg.as_str() {
            if let Some(resolved) = resolve_today_token(s, today) {
                return Value::String(resolved);
            }
        }
    }
    arg.clone()
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

fn field_value(entity: &Entity, field: &str) -> Value {
    match field {
        "created_at" => Value::String(entity.created_at.clone()),
        "updated_at" => Value::String(entity.updated_at.clone()),
        _ => entity.data.get(field).cloned().unwrap_or(Value::Null),
    }
}

fn build_chart_series(block: &ViewBlock, entities: &[Entity]) -> Vec<ChartSeries> {
    let x = block
        .x
        .as_deref()
        .expect("chart x validated before shaping");
    let y = block
        .y
        .as_deref()
        .expect("chart y validated before shaping");
    let mut series = IndexMap::<String, Vec<ChartPoint>>::new();

    for entity in entities {
        let y_value = field_value(entity, y);
        let Some(y) = extract_f64(&y_value) else {
            continue;
        };
        let name = block.series.as_deref().map_or_else(
            || block.view.clone(),
            |series_field| match field_value(entity, series_field) {
                Value::String(name) => name,
                _ => String::new(),
            },
        );
        series.entry(name).or_default().push(ChartPoint {
            x: field_value(entity, x),
            y,
        });
    }

    series
        .into_iter()
        .map(|(name, points)| ChartSeries { name, points })
        .collect()
}

fn compute_aggregate(
    view: &str,
    expr: &str,
    schema: &ResolvedSchema,
    entities: &[Entity],
) -> Result<Value, ViewError> {
    let (func, field) = parse_agg(expr).ok_or_else(|| ViewError::BadAggregate {
        view: view.to_string(),
        expr: expr.to_string(),
    })?;
    if !matches!(func, "count" | "sum" | "min" | "max" | "avg") {
        return Err(ViewError::BadAggregate {
            view: view.to_string(),
            expr: expr.to_string(),
        });
    }
    if !schema.fields.contains_key(field) {
        return Err(ViewError::UnknownField {
            view: view.to_string(),
            source: schema.name.clone(),
            field: field.to_string(),
        });
    }
    if matches!(func, "sum" | "min" | "max" | "avg") {
        validate_money_currency(view, field, schema, entities)?;
    }

    match func {
        "count" => Ok(Value::from(entities.len())),
        "sum" => Ok(Value::from(
            entities
                .iter()
                .filter_map(|entity| entity.data.get(field).and_then(extract_f64))
                .sum::<f64>(),
        )),
        "min" => Ok(numeric_extreme(field, entities, f64::min)),
        "max" => Ok(numeric_extreme(field, entities, f64::max)),
        "avg" => Ok(numeric_mean(field, entities)),
        _ => unreachable!("aggregate function validated before field lookup"),
    }
}

fn parse_agg(expr: &str) -> Option<(&str, &str)> {
    let expr = expr.trim();
    let open = expr.find('(')?;
    let inner = expr.strip_suffix(')')?;
    let func = inner[..open].trim();
    let field = inner[open + 1..].trim();
    if func.is_empty() || field.is_empty() || field.contains('(') || field.contains(')') {
        return None;
    }
    Some((func, field))
}

fn validate_money_currency(
    view: &str,
    field: &str,
    schema: &ResolvedSchema,
    entities: &[Entity],
) -> Result<(), ViewError> {
    if !matches!(&schema.fields[field].kind, FieldKind::Money) {
        return Ok(());
    }

    let mut currency = None;
    for entity in entities {
        let Some(value) = entity.data.get(field) else {
            continue;
        };
        if extract_f64(value).is_none() {
            continue;
        }
        let Some(next) = value
            .as_object()
            .and_then(|object| object.get("currency"))
            .and_then(Value::as_str)
            .filter(|currency| !currency.is_empty())
        else {
            continue;
        };
        if let Some(current) = currency {
            if current != next {
                return Err(ViewError::CurrencyMismatch {
                    view: view.to_string(),
                    field: field.to_string(),
                });
            }
        } else {
            currency = Some(next);
        }
    }
    Ok(())
}

fn numeric_extreme(field: &str, entities: &[Entity], combine: impl Fn(f64, f64) -> f64) -> Value {
    entities
        .iter()
        .filter_map(|entity| entity.data.get(field).and_then(extract_f64))
        .fold(None, |acc: Option<f64>, value| {
            Some(acc.map_or(value, |current| combine(current, value)))
        })
        .map(Value::from)
        .unwrap_or(Value::Null)
}

fn numeric_mean(field: &str, entities: &[Entity]) -> Value {
    let values: Vec<f64> = entities
        .iter()
        .filter_map(|entity| entity.data.get(field).and_then(extract_f64))
        .collect();
    if values.is_empty() {
        Value::Null
    } else {
        Value::from(values.iter().sum::<f64>() / values.len() as f64)
    }
}

/// 엔티티 슬라이스를 `sort` 스펙(`-` 접두사는 내림차순)에 따라 정렬한다.
/// 뷰 엔진 내부와 서버의 자동 기본 뷰(GET /api/entities)가 동일한 정렬 규칙을 공유하기 위해 공개됨.
pub fn sort_entities(entities: &mut [Entity], schema: &ResolvedSchema, sort: &str) {
    let (descending, field) = match sort.strip_prefix('-') {
        Some(field) => (true, field),
        None => (false, sort),
    };
    if is_system_column(field) {
        entities.sort_by(|l, r| {
            let (a, b) = if field == "updated_at" {
                (&l.updated_at, &r.updated_at)
            } else {
                (&l.created_at, &r.created_at)
            };
            let ord = a.cmp(b);
            if descending {
                ord.reverse()
            } else {
                ord
            }
        });
        return;
    }
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
    use crate::view::ChartType;
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

    fn measure_schemas() -> SchemaSet {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("측정.yaml"),
            "type: 측정\nfields:\n  지표: { kind: enum, options: [체중, 수면시간] }\n  값: { kind: number, required: true }\n  시각: { kind: date }\n",
        )
        .unwrap();
        SchemaSet::load_dir(dir.path()).unwrap()
    }

    async fn seed_measure(store: &EntityStore, s: &SchemaSet) {
        store
            .create(
                s,
                "측정",
                obj(json!({ "지표": "체중", "값": 82.0, "시각": "2026-07-01" })),
            )
            .await
            .unwrap();
        store
            .create(
                s,
                "측정",
                obj(json!({ "지표": "체중", "값": 81.0, "시각": "2026-07-05" })),
            )
            .await
            .unwrap();
        store
            .create(
                s,
                "측정",
                obj(json!({ "지표": "수면시간", "값": 6.5, "시각": "2026-07-05" })),
            )
            .await
            .unwrap();
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
    async fn 집계_sum_count_min_max() {
        let s = schemas();
        let store = EntityStore::open_in_memory().await.unwrap();
        seed(&store, &s).await;
        // 상태=주문됨(A 30만, C 65만) 합계 95만, count 2, min 30만, max 65만
        let b = block("view: v\nsource: 물건\nfilter: { 상태: 주문됨 }\naggregate: { 합계: sum(가격), 건수: count(가격), 최소: min(가격), 최대: max(가격) }\n");
        let r = run_view(&store, &s, &b).await.unwrap();
        assert_eq!(r.aggregates["합계"], serde_json::json!(950000.0));
        assert_eq!(r.aggregates["건수"], serde_json::json!(2));
        assert_eq!(r.aggregates["최소"], serde_json::json!(300000.0));
        assert_eq!(r.aggregates["최대"], serde_json::json!(650000.0));
    }

    #[tokio::test]
    async fn avg_집계_평균과_빈집합() {
        let s = schemas();
        let store = EntityStore::open_in_memory().await.unwrap();
        seed(&store, &s).await;

        let r = run_view(
            &store,
            &s,
            &block("view: v\nsource: 물건\nfilter: { 상태: 주문됨 }\naggregate: { 평균가: avg(가격) }\n"),
        )
        .await
        .unwrap();
        assert_eq!(r.aggregates["평균가"], json!(475000.0));

        let empty = run_view(
            &store,
            &s,
            &block(
                "view: v\nsource: 물건\nfilter: { 상태: 과거 }\naggregate: { 평균: avg(가격) }\n",
            ),
        )
        .await
        .unwrap();
        assert_eq!(empty.aggregates["평균"], Value::Null);
    }

    #[tokio::test]
    async fn chart_셰이핑_지표별_계열() {
        let s = measure_schemas();
        let store = EntityStore::open_in_memory().await.unwrap();
        seed_measure(&store, &s).await;

        let r = run_view(
            &store,
            &s,
            &block("view: 추세\nsource: 측정\nlayout: chart\nx: 시각\ny: 값\nseries: 지표\nsort: 시각\n"),
        )
        .await
        .unwrap();

        let chart = r.chart.as_ref().unwrap();
        assert_eq!(chart.len(), 2);
        assert_eq!(chart[0].name, "체중");
        assert_eq!(chart[0].points.len(), 2);
        assert_eq!(chart[0].points[0].x, json!("2026-07-01"));
        assert_eq!(chart[0].points[0].y, 82.0);
        assert_eq!(chart[1].name, "수면시간");
        assert_eq!(chart[1].points.len(), 1);
    }

    #[tokio::test]
    async fn chart_type은_실행_결과에_전달된다() {
        let s = measure_schemas();
        let store = EntityStore::open_in_memory().await.unwrap();
        seed_measure(&store, &s).await;

        let r = run_view(
            &store,
            &s,
            &block("view: 추세\nsource: 측정\nlayout: chart\nchart_type: bar\nx: 시각\ny: 값\n"),
        )
        .await
        .unwrap();
        assert_eq!(r.chart_type, Some(ChartType::Bar));
        assert_eq!(
            serde_json::to_value(&r).unwrap()["chart_type"],
            json!("bar")
        );

        let table = run_view(&store, &s, &block("view: 표\nsource: 측정\n"))
            .await
            .unwrap();
        assert_eq!(table.chart_type, None);
        assert!(serde_json::to_value(&table)
            .unwrap()
            .get("chart_type")
            .is_none());
    }

    #[tokio::test]
    async fn chart_x축_없으면_에러() {
        let s = measure_schemas();
        let store = EntityStore::open_in_memory().await.unwrap();
        let err = run_view(
            &store,
            &s,
            &block("view: 추세\nsource: 측정\nlayout: chart\ny: 값\n"),
        )
        .await
        .unwrap_err();

        assert!(
            matches!(&err, ViewError::MissingChartAxis { axis: "x", .. }),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn count는_필드_존재가_아닌_매칭_행_수를_센다() {
        let s = schemas();
        let store = EntityStore::open_in_memory().await.unwrap();
        seed(&store, &s).await;
        store
            .create(
                &s,
                "물건",
                obj(json!({ "이름": "D", "상태": "주문됨", "배송예정일": "2026-07-22" })),
            )
            .await
            .unwrap();

        let r = run_view(
            &store,
            &s,
            &block("view: v\nsource: 물건\nfilter: { 상태: 주문됨 }\naggregate: { 건수: count(가격) }\n"),
        )
        .await
        .unwrap();

        assert_eq!(r.entities.len(), 3);
        assert_eq!(r.aggregates["건수"], serde_json::json!(3));
    }

    #[tokio::test]
    async fn 집계_없는_필드는_unknownfield() {
        let s = schemas();
        let store = EntityStore::open_in_memory().await.unwrap();
        let b = block("view: v\nsource: 물건\naggregate: { 합계: sum(유령) }\n");
        let err = run_view(&store, &s, &b).await.unwrap_err();
        assert!(
            matches!(&err, ViewError::UnknownField { field, .. } if field == "유령"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn 잘못된_집계식_빈필드와_닫히지_않은_괄호는_badaggregate() {
        let s = schemas();
        let store = EntityStore::open_in_memory().await.unwrap();

        for expr in ["sum()", "sum(가격"] {
            let b = block(&format!(
                "view: v\nsource: 물건\naggregate: {{ 합계: {expr} }}\n"
            ));
            let err = run_view(&store, &s, &b).await.unwrap_err();
            assert!(
                matches!(
                    &err,
                    ViewError::BadAggregate { expr: bad_expr, .. }
                        if bad_expr.as_str() == expr
                ),
                "unexpected error for {expr}: {err}"
            );
        }
    }

    #[tokio::test]
    async fn money_집계는_통화가_섞이면_에러() {
        let s = schemas();
        let store = EntityStore::open_in_memory().await.unwrap();
        store
            .create(
                &s,
                "물건",
                obj(json!({ "이름": "KRW", "상태": "주문됨", "가격": {"amount": 300000.0, "currency": "KRW"} })),
            )
            .await
            .unwrap();
        store
            .create(
                &s,
                "물건",
                obj(json!({ "이름": "USD", "상태": "주문됨", "가격": {"amount": 200.0, "currency": "USD"} })),
            )
            .await
            .unwrap();

        let b = block(
            "view: v\nsource: 물건\nfilter: { 상태: 주문됨 }\naggregate: { 합계: sum(가격) }\n",
        );
        let err = run_view(&store, &s, &b).await.unwrap_err();
        assert!(
            matches!(&err, ViewError::CurrencyMismatch { field, .. } if field == "가격"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn 집계는_정렬_방향과_무관하게_같은_필터집합을_사용한다() {
        let s = schemas();
        let store = EntityStore::open_in_memory().await.unwrap();
        seed(&store, &s).await;

        let asc = run_view(
            &store,
            &s,
            &block("view: v\nsource: 물건\nsort: 가격\naggregate: { 합계: sum(가격) }\n"),
        )
        .await
        .unwrap();
        let desc = run_view(
            &store,
            &s,
            &block("view: v\nsource: 물건\nsort: -가격\naggregate: { 합계: sum(가격) }\n"),
        )
        .await
        .unwrap();

        let asc_names: Vec<&str> = asc
            .entities
            .iter()
            .map(|e| e.data["이름"].as_str().unwrap())
            .collect();
        let desc_names: Vec<&str> = desc
            .entities
            .iter()
            .map(|e| e.data["이름"].as_str().unwrap())
            .collect();
        assert_eq!(asc_names, ["B", "A", "C"]);
        assert_eq!(desc_names, ["C", "A", "B"]);
        assert_eq!(asc.aggregates["합계"], serde_json::json!(1050000.0));
        assert_eq!(desc.aggregates["합계"], asc.aggregates["합계"]);
    }

    #[tokio::test]
    async fn 잘못된_집계식은_에러() {
        let s = schemas();
        let store = EntityStore::open_in_memory().await.unwrap();
        let b = block("view: v\nsource: 물건\naggregate: { 합계: total(가격) }\n");
        assert!(run_view(&store, &s, &b).await.is_err());
    }

    #[tokio::test]
    async fn 지원하지_않는_집계함수는_필드검증보다_먼저_badaggregate() {
        let s = schemas();
        let store = EntityStore::open_in_memory().await.unwrap();
        let b = block("view: v\nsource: 물건\naggregate: { 합계: total(유령) }\n");
        let err = run_view(&store, &s, &b).await.unwrap_err();
        assert!(
            matches!(&err, ViewError::BadAggregate { expr, .. } if expr == "total(유령)"),
            "unexpected error: {err}"
        );
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

    fn d(s: &str) -> chrono::NaiveDate {
        chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[tokio::test]
    async fn lte_gte와_today_토큰() {
        let s = schemas();
        let store = EntityStore::open_in_memory().await.unwrap();
        seed(&store, &s).await; // 배송예정일: A=07-10, B=08-01, C=07-20
        let r = run_view_at(
            &store,
            &s,
            &block("view: v\nsource: 물건\nfilter: { 배송예정일: { lte: $today+7d } }\n"),
            d("2026-07-03"),
        )
        .await
        .unwrap();
        let names: Vec<&str> = r
            .entities
            .iter()
            .map(|e| e.data["이름"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["A"]); // 07-10 ≤ 07-10

        let r = run_view_at(
            &store,
            &s,
            &block("view: v\nsource: 물건\nfilter: { 배송예정일: { gte: $today } }\n"),
            d("2026-07-21"),
        )
        .await
        .unwrap();
        assert_eq!(r.entities.len(), 1); // B(08-01)만
    }

    #[tokio::test]
    async fn today_토큰을_비date_필드에_쓰면_에러() {
        let s = schemas();
        let store = EntityStore::open_in_memory().await.unwrap();
        let err = run_view_at(
            &store,
            &s,
            &block("view: v\nsource: 물건\nfilter: { 이름: { lte: $today } }\n"),
            d("2026-07-03"),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("$today"));
    }

    #[test]
    fn 토큰_해석() {
        assert_eq!(
            resolve_today_token("$today", d("2026-07-03")).unwrap(),
            "2026-07-03"
        );
        assert_eq!(
            resolve_today_token("$today+7d", d("2026-07-03")).unwrap(),
            "2026-07-10"
        );
        assert_eq!(
            resolve_today_token("$today-3d", d("2026-07-03")).unwrap(),
            "2026-06-30"
        );
        assert!(resolve_today_token("$today+7", d("2026-07-03")).is_none()); // d 누락
        assert!(resolve_today_token("어제", d("2026-07-03")).is_none());
    }

    #[tokio::test]
    async fn limit은_정렬_후_상위_n행() {
        let s = schemas();
        let store = EntityStore::open_in_memory().await.unwrap();
        seed(&store, &s).await;
        let r = run_view_at(
            &store,
            &s,
            &block("view: v\nsource: 물건\nsort: -가격\nlimit: 2\n"),
            d("2026-07-03"),
        )
        .await
        .unwrap();
        let names: Vec<&str> = r
            .entities
            .iter()
            .map(|e| e.data["이름"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["C", "A"]);
    }

    #[tokio::test]
    async fn 시스템컬럼_updated_at_정렬() {
        let s = schemas();
        let store = EntityStore::open_in_memory().await.unwrap();
        let a = store
            .create(&s, "물건", obj(json!({ "이름": "먼저" })))
            .await
            .unwrap();
        store
            .create(&s, "물건", obj(json!({ "이름": "나중" })))
            .await
            .unwrap();
        store
            .update(&s, &a.id, obj(json!({ "이름": "먼저(수정)" })))
            .await
            .unwrap(); // 먼저를 최신으로
        let r = run_view_at(
            &store,
            &s,
            &block("view: v\nsource: 물건\nsort: \"-updated_at\"\n"),
            d("2026-07-03"),
        )
        .await
        .unwrap();
        assert_eq!(r.entities[0].data["이름"], json!("먼저(수정)"));
    }

    #[tokio::test]
    async fn viewresult는_source_filter_sort를_에코한다() {
        let s = schemas();
        let store = EntityStore::open_in_memory().await.unwrap();
        let r = run_view_at(
            &store,
            &s,
            &block("view: v\nsource: 물건\nfilter: { 상태: 주문됨 }\nsort: 배송예정일\n"),
            d("2026-07-03"),
        )
        .await
        .unwrap();
        assert_eq!(r.source, "물건");
        assert_eq!(r.sort.as_deref(), Some("배송예정일"));
        assert_eq!(r.filter.as_ref().unwrap()["상태"], json!("주문됨"));
    }
}
