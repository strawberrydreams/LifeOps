use lifeops_core::entity::EntityStore;
use lifeops_core::schema::{FieldKind, SchemaSet};
use lifeops_core::view::{Layout, PageDef};
use serde_json::{json, Map, Value};

fn obj(v: Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}

#[tokio::test]
async fn 시드_스키마로_전체_시나리오() {
    // 저장소 루트의 schemas/ 디렉터리를 그대로 로드한다
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
    let schemas = SchemaSet::load_dir(&dir).expect("시드 스키마가 유효해야 함");
    for ty in ["물건", "시계", "노트", "할일"] {
        assert!(schemas.get(ty).is_some(), "{ty} 스키마가 있어야 함");
    }
    // 시계는 물건을 상속
    assert!(schemas.get("시계").unwrap().fields.contains_key("가격"));

    let store = EntityStore::open_in_memory().await.unwrap();
    let watch = store
        .create(
            &schemas,
            "시계",
            obj(json!({
                "이름": "SEIKO x Hatsune Miku LAWSON 50주년",
                "상태": "위시",
                "가격": { "amount": 650000.0, "currency": "KRW" },
                "콜라보": "하츠네 미쿠"
            })),
        )
        .await
        .unwrap();

    // 위시 → 주문됨 (한 곳만 수정)
    store
        .update(
            &schemas,
            &watch.id,
            obj(json!({ "상태": "주문됨", "배송예정일": "2026-09-15" })),
        )
        .await
        .unwrap();

    // 할일이 시계를 참조
    let todo = store
        .create(
            &schemas,
            "할일",
            obj(json!({ "내용": "배송대행 신청", "관련": [watch.id.clone()] })),
        )
        .await
        .unwrap();

    // 역링크로 연결 확인, 삭제 보호 동작
    let back = store.backlinks(&watch.id).await.unwrap();
    assert_eq!(back[0].from_id, todo.id);
    assert!(store.delete(&watch.id).await.is_err());

    // 물건 family 조회에 시계가 포함된다
    let items = store.list(&schemas.family_of("물건")).await.unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].data["상태"], "주문됨");
}

#[test]
fn 측정_시드_스키마와_건강_페이지가_유효하다() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("측정.yaml"),
        r#"type: 측정
category: 기록
fields:
  지표: { kind: enum, options: [체중, 수면시간, 에너지, 스트레스, 잔고], required: true }
  값:   { kind: number, required: true }
  단위: { kind: text }
  시각: { kind: date, required: true }
  메모: { kind: text }
"#,
    )
    .unwrap();

    let schemas = SchemaSet::load_dir(dir.path()).unwrap();
    let measurement = schemas.get("측정").expect("측정 스키마 필요");
    assert_eq!(measurement.category.as_deref(), Some("기록"));
    assert_eq!(measurement.fields["지표"].kind.to_string(), "enum");
    assert_eq!(measurement.fields["값"].kind.to_string(), "number");
    assert_eq!(measurement.fields["시각"].kind.to_string(), "date");
    assert!(measurement.fields["지표"].required);
    assert!(measurement.fields["값"].required);
    assert!(measurement.fields["시각"].required);

    let page: PageDef = serde_yaml::from_str(
        r#"page: 건강
blocks:
  - view: 체중 추세
    source: 측정
    filter: { 지표: 체중 }
    sort: 시각
    layout: chart
    x: 시각
    y: 값
"#,
    )
    .unwrap();
    assert_eq!(page.page, "건강");
    assert_eq!(page.blocks.len(), 1);
    assert_eq!(page.blocks[0].source, "측정");
    assert_eq!(page.blocks[0].layout, Layout::Chart);
    assert_eq!(page.blocks[0].x.as_deref(), Some("시각"));
    assert_eq!(page.blocks[0].y.as_deref(), Some("값"));
}

#[test]
fn 실제_시드_스키마와_카테고리와_홈페이지가_로드된다() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let schemas = lifeops_core::schema::SchemaSet::load_dir(&root.join("schemas")).unwrap();
    for ty in [
        "노트",
        "물건",
        "시계",
        "할일",
        "계좌",
        "거래",
        "구독",
        "공간",
        "소모품",
        "활동",
        "측정",
    ] {
        assert!(schemas.get(ty).is_some(), "시드 타입 누락: {ty}");
    }
    let measurement = schemas.get("측정").unwrap();
    assert_eq!(measurement.category.as_deref(), Some("기록"));
    for field in ["지표", "값", "단위", "시각", "메모"] {
        assert!(
            measurement.fields.contains_key(field),
            "측정 필드 누락: {field}"
        );
    }
    let metric = &measurement.fields["지표"];
    assert_eq!(metric.kind, FieldKind::Enum);
    assert_eq!(
        metric.options.as_deref(),
        Some(
            &[
                "체중".to_string(),
                "수면시간".to_string(),
                "에너지".to_string(),
                "스트레스".to_string(),
                "잔고".to_string(),
            ][..]
        )
    );
    assert!(metric.required);
    assert_eq!(measurement.fields["값"].kind, FieldKind::Number);
    assert!(measurement.fields["값"].required);
    assert_eq!(measurement.fields["단위"].kind, FieldKind::Text);
    assert!(!measurement.fields["단위"].required);
    assert_eq!(measurement.fields["시각"].kind, FieldKind::Date);
    assert!(measurement.fields["시각"].required);
    assert_eq!(measurement.fields["메모"].kind, FieldKind::Text);
    assert!(!measurement.fields["메모"].required);
    assert_eq!(
        schemas.get("시계").unwrap().category.as_deref(),
        Some("컬렉션")
    ); // 상속
    assert!(schemas
        .get("할일")
        .unwrap()
        .behaviors
        .as_ref()
        .unwrap()
        .recurrence
        .is_some());

    let cats = lifeops_core::schema::load_categories(&root.join("categories.yaml")).unwrap();
    let names: Vec<&str> = cats.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        names,
        ["할일", "메모", "컬렉션", "재무", "환경", "취미", "기록"]
    );
    let records = cats
        .iter()
        .find(|c| c.name == "기록")
        .expect("기록 카테고리 필요");
    assert_eq!(records.icon.as_deref(), Some("📈"));
    assert_eq!(
        records.description.as_deref(),
        Some("시간에 따라 기록하는 측정값")
    );

    let pages = lifeops_core::view::PageSet::load_dir(&root.join("views")).unwrap();
    let home = pages.get("홈").expect("views/홈.yaml 필요");
    assert_eq!(home.blocks.len(), 3);
    let health = pages.get("건강").expect("views/건강.yaml 필요");
    assert_eq!(health.blocks.len(), 3);

    let quick = &health.blocks[0];
    assert_eq!(quick.view, "빠른 기록");
    assert_eq!(quick.source, "측정");
    assert_eq!(quick.layout, Layout::Record);

    let weight = &health.blocks[1];
    assert_eq!(weight.view, "체중 추세");
    assert_eq!(weight.source, "측정");
    assert_eq!(
        weight.filter.as_ref().expect("체중 추세 filter 필요")["지표"],
        json!("체중")
    );
    assert_eq!(weight.sort.as_deref(), Some("시각"));
    assert_eq!(weight.layout, Layout::Chart);
    assert_eq!(weight.x.as_deref(), Some("시각"));
    assert_eq!(weight.y.as_deref(), Some("값"));

    let sleep = &health.blocks[2];
    assert_eq!(sleep.view, "최근 7일 평균 수면");
    assert_eq!(sleep.source, "측정");
    let sleep_filter = sleep.filter.as_ref().expect("수면 평균 filter 필요");
    assert_eq!(sleep_filter["지표"], json!("수면시간"));
    assert_eq!(sleep_filter["시각"]["gte"], json!("$today-7d"));
    assert_eq!(
        sleep.aggregate.as_ref().expect("수면 평균 aggregate 필요")["평균수면"],
        "avg(값)"
    );
}
