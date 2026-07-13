use crate::import::coerce::coerce;
use crate::import::map::{FileCtx, Source};
use crate::import::rules::{matches, Action, RuleSet};
use crate::import::{canonicalize_hangul, hangul_eq, parse_tables, ParsedDoc};
use crate::schema::{FieldKind, ResolvedSchema, SchemaSet};
use serde_json::{json, Map, Value};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct PlannedEntity {
    pub entity_type: String,
    pub src_key: String,
    pub data: Map<String, Value>,
}

#[derive(Debug, Default)]
pub struct RouteResult {
    pub skipped: bool,
    pub hit_default: bool,
    pub dropped_required: usize,
    pub price_warnings: usize,
    pub config_errors: Vec<String>,
    pub entities: Vec<PlannedEntity>,
}

pub fn route_file(
    rules: &RuleSet,
    schemas: &SchemaSet,
    relpath: &str,
    doc: &ParsedDoc,
) -> RouteResult {
    let filename = file_stem(relpath);
    let dirs = ancestor_dirs(relpath);
    let context = FileCtx {
        frontmatter: &doc.frontmatter,
        body: &doc.body,
        filename: &filename,
        dirs: &dirs,
    };
    let Some((rule_index, rule)) = rules
        .rules
        .iter()
        .enumerate()
        .find(|(_, rule)| matches(&rule.matcher, relpath, &doc.frontmatter))
    else {
        return RouteResult::default();
    };
    let mut result = RouteResult {
        hit_default: rule.is_default,
        ..RouteResult::default()
    };
    result.config_errors = validate_rule_schema(rule_index, rule, schemas);
    if !result.config_errors.is_empty() {
        return result;
    }

    match &rule.action {
        Action::Skip => result.skipped = true,
        Action::Single { map } => {
            let Some((entity_type, schema)) = target_schema(rule.to.as_deref(), schemas) else {
                return result;
            };
            let fields = mapped_fields(map, &context, schema);
            let src_key = single_src_key(relpath, doc);
            push_finalized(
                &mut result,
                entity_type,
                schema,
                fields,
                src_key,
                &rule.provenance,
            );
        }
        Action::Table { cols, set } => {
            let Some((entity_type, schema)) = target_schema(rule.to.as_deref(), schemas) else {
                return result;
            };
            let mut seen_slugs = HashMap::<String, usize>::new();
            for table in parse_tables(&doc.body) {
                let columns: Vec<(usize, &String)> = cols
                    .iter()
                    .filter(|(_, field)| schema.fields.get(field.as_str()).is_some())
                    .filter_map(|(candidates, field)| {
                        table
                            .headers
                            .iter()
                            .position(|header| {
                                candidates
                                    .iter()
                                    .any(|candidate| hangul_eq(header, candidate))
                            })
                            .map(|index| (index, field))
                    })
                    .collect();
                if columns.is_empty() {
                    continue;
                }

                for row in &table.rows {
                    let mut fields = Map::new();
                    let mut pending_price_notes = Vec::new();
                    for (index, field) in &columns {
                        let Some(raw) = row.get(*index).filter(|raw| !raw.is_empty()) else {
                            continue;
                        };
                        let Some(field_def) = schema.fields.get(field.as_str()) else {
                            continue;
                        };
                        match coerce(&Value::String(raw.clone()), field_def) {
                            Some(value) => {
                                fields.insert((*field).clone(), value);
                            }
                            None if field_def.kind == FieldKind::Money => {
                                result.price_warnings += 1;
                                pending_price_notes.push(raw.clone());
                            }
                            None => {}
                        }
                    }
                    for (field, source) in set {
                        if fields.contains_key(field) {
                            continue;
                        }
                        if let Some(value) = eval_field(source, &context, schema, field) {
                            fields.insert(field.clone(), value);
                        }
                    }
                    append_price_notes(&mut fields, schema, &pending_price_notes);

                    let Some(name) = fields
                        .get("이름")
                        .and_then(Value::as_str)
                        .filter(|name| !name.trim().is_empty())
                    else {
                        result.dropped_required += 1;
                        continue;
                    };
                    let slug = row_slug(name);
                    let count = seen_slugs.entry(slug.clone()).or_default();
                    *count += 1;
                    let suffix = if *count == 1 {
                        String::new()
                    } else {
                        format!("-{count}")
                    };
                    let src_key = format!("row:{relpath}#{slug}{suffix}");
                    push_finalized(
                        &mut result,
                        entity_type,
                        schema,
                        fields,
                        src_key,
                        &rule.provenance,
                    );
                }
            }
        }
    }
    result
}

pub fn validate_rules_schema(rules: &RuleSet, schemas: &SchemaSet) -> Vec<String> {
    rules
        .rules
        .iter()
        .enumerate()
        .flat_map(|(index, rule)| validate_rule_schema(index, rule, schemas))
        .collect()
}

fn validate_rule_schema(
    rule_index: usize,
    rule: &crate::import::rules::Rule,
    schemas: &SchemaSet,
) -> Vec<String> {
    let mut errors = Vec::new();
    let (mapping_fields, entity_type): (Vec<&String>, Option<&str>) = match &rule.action {
        Action::Skip => return errors,
        Action::Single { map } => (
            map.iter().map(|(field, _)| field).collect(),
            rule.to.as_deref(),
        ),
        Action::Table { cols, set } => (
            cols.iter()
                .map(|(_, field)| field)
                .chain(set.iter().map(|(field, _)| field))
                .collect(),
            rule.to.as_deref(),
        ),
    };

    let Some(entity_type) = entity_type.filter(|entity_type| !entity_type.trim().is_empty()) else {
        errors.push(format!("rules[{rule_index}]: 대상 schema 'to'가 필요함"));
        return errors;
    };
    let Some(schema) = schemas.get(entity_type) else {
        errors.push(format!(
            "rules[{rule_index}]: 알 수 없는 schema '{entity_type}'"
        ));
        return errors;
    };
    for field in mapping_fields {
        if !schema.fields.contains_key(field.as_str()) {
            errors.push(format!(
                "rules[{rule_index}]: schema '{entity_type}'에 field '{field}'가 없음"
            ));
        }
    }
    errors
}

fn target_schema<'a>(
    entity_type: Option<&'a str>,
    schemas: &'a SchemaSet,
) -> Option<(&'a str, &'a ResolvedSchema)> {
    let entity_type = entity_type?;
    Some((entity_type, schemas.get(entity_type)?))
}

fn mapped_fields(
    mapping: &[(String, Source)],
    context: &FileCtx<'_>,
    schema: &ResolvedSchema,
) -> Map<String, Value> {
    mapping
        .iter()
        .filter_map(|(field, source)| {
            eval_field(source, context, schema, field).map(|value| (field.clone(), value))
        })
        .collect()
}

fn eval_field(
    source: &Source,
    context: &FileCtx<'_>,
    schema: &ResolvedSchema,
    field: &str,
) -> Option<Value> {
    let field_def = schema.fields.get(field)?;
    coerce(&source.eval(context)?, field_def)
}

fn append_price_notes(
    fields: &mut Map<String, Value>,
    schema: &ResolvedSchema,
    price_notes: &[String],
) {
    let Some(field_def) = schema.fields.get("메모") else {
        return;
    };
    if price_notes.is_empty() {
        return;
    }

    let existing = match fields.get("메모") {
        Some(Value::String(existing)) => Some(existing.as_str()),
        Some(_) => return,
        None => None,
    };
    let merged = match existing {
        Some(existing) if !existing.is_empty() => {
            format!("{existing}\n{}", price_notes.join("\n"))
        }
        _ => price_notes.join("\n"),
    };
    if let Some(value) = coerce(&Value::String(merged), field_def) {
        fields.insert("메모".to_string(), value);
    }
}

fn push_finalized(
    result: &mut RouteResult,
    entity_type: &str,
    schema: &ResolvedSchema,
    fields: Map<String, Value>,
    src_key: String,
    provenance: &str,
) {
    if let Some(entity) = finalize(entity_type, schema, fields, src_key, provenance) {
        result.entities.push(entity);
    } else {
        result.dropped_required += 1;
    }
}

fn finalize(
    entity_type: &str,
    schema: &ResolvedSchema,
    fields: Map<String, Value>,
    src_key: String,
    provenance: &str,
) -> Option<PlannedEntity> {
    if schema
        .fields
        .iter()
        .any(|(name, field)| field.required && !fields.contains_key(name))
    {
        return None;
    }

    let mut data = fields;
    let meta = data
        .keys()
        .filter(|name| schema.fields.contains_key(name.as_str()))
        .map(|name| (name.clone(), json!({ "source": provenance })))
        .collect();
    data.insert("$src".to_string(), json!(src_key));
    data.insert("$meta".to_string(), Value::Object(meta));
    Some(PlannedEntity {
        entity_type: entity_type.to_string(),
        src_key,
        data,
    })
}

fn single_src_key(relpath: &str, doc: &ParsedDoc) -> String {
    let is_x = doc.frontmatter.get("source").and_then(Value::as_str) == Some("x");
    let tweet_id = doc.frontmatter.get("tweet_id").and_then(nonempty_scalar);
    if is_x && doc.frontmatter_ok {
        if let Some(tweet_id) = tweet_id {
            return format!("x:{tweet_id}");
        }
    }
    format!("path:{relpath}")
}

fn nonempty_scalar(value: &Value) -> Option<String> {
    let value = match value {
        Value::String(value) => value.trim().to_string(),
        Value::Number(value) => value.to_string(),
        _ => return None,
    };
    (!value.is_empty()).then_some(value)
}

fn row_slug(name: &str) -> String {
    canonicalize_hangul(name)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn file_stem(relpath: &str) -> String {
    let name = relpath.rsplit('/').next().unwrap_or(relpath);
    name.rsplit_once('.')
        .map_or_else(|| name.to_string(), |(stem, _)| stem.to_string())
}

fn ancestor_dirs(relpath: &str) -> Vec<String> {
    let mut parts: Vec<&str> = relpath.split('/').collect();
    parts.pop();
    parts.into_iter().rev().map(ToString::to_string).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::{parse_document, RuleSet};
    use crate::schema::SchemaSet;

    fn schemas() -> SchemaSet {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("노트.yaml"), "type: 노트\nfields:\n  제목: { kind: text, required: true }\n  본문: { kind: richtext }\n  태그: { kind: \"list<text>\" }\n  출처: { kind: text }\n  url: { kind: url }\n").unwrap();
        std::fs::write(dir.path().join("물건.yaml"), "type: 물건\nfields:\n  이름: { kind: text, required: true }\n  가격: { kind: money }\n  구매링크: { kind: url }\n  상태: { kind: enum, options: [위시, 보유] }\n  카테고리: { kind: enum, options: [시계, 전자제품] }\n  메모: { kind: text }\n").unwrap();
        SchemaSet::load_dir(dir.path()).unwrap()
    }

    fn rules() -> RuleSet {
        RuleSet::from_yaml(
            r#"
rules:
  - match: { fm.source: x }
    to: 노트
    map:
      제목: "fm.aliases[0] | fm.title | filename"
      본문: body
      출처: "fm.handle | fm.author"
      url: fm.url
    provenance: imported:x
  - match: { path: "*위시리스트*" }
    to: 물건
    rows: table
    cols:
      "제품|분류": 이름
      가격: 가격
      가격2: 가격
      링크: 구매링크
      비고: 메모
    set:
      상태: '"위시"'
      카테고리: dirs
    provenance: imported
  - match: { fm.type: [moc, dashboard] }
    skip: true
  - default:
      to: 노트
      map:
        제목: "fm.title | filename"
        본문: body
        태그: fm.tags
      provenance: imported
"#,
        )
        .unwrap()
    }

    #[test]
    fn x_북마크는_노트로_출처url_프로비넌스_출처키까지() {
        let doc = parse_document("---\nsource: x\nhandle: ArchiveExplorer\ntweet_id: 2071192832455430283\naliases: [\"루프와 하니스\"]\nurl: https://x.com/ArchiveExplorer/status/2071192832455430283\n---\n본문 텍스트");
        let r = route_file(&rules(), &schemas(), "Obsidian/X/루프.md", &doc);
        let e = &r.entities[0];
        assert_eq!(e.entity_type, "노트");
        assert_eq!(e.src_key, "x:2071192832455430283");
        assert_eq!(e.data["제목"], serde_json::json!("루프와 하니스"));
        assert_eq!(e.data["출처"], serde_json::json!("ArchiveExplorer"));
        assert_eq!(
            e.data["url"],
            serde_json::json!("https://x.com/ArchiveExplorer/status/2071192832455430283")
        );
        assert_eq!(e.data["$src"], serde_json::json!("x:2071192832455430283"));
        assert_eq!(
            e.data["$meta"]["제목"]["source"],
            serde_json::json!("imported:x")
        );
        assert_eq!(
            e.data["$meta"]["url"]["source"],
            serde_json::json!("imported:x")
        );
        assert!(e.data["$meta"].get("$src").is_none());
    }

    #[test]
    fn reference는_default_노트이고_tweet_id만으로_x키가_되지_않는다() {
        let doc = parse_document("---\ntitle: 사용자 권한\ntags: [linux/시스템]\ntype: reference\ntweet_id: 123\n---\nchmod chown");
        let r = route_file(&rules(), &schemas(), "Obsidian/Linux/권한.md", &doc);
        assert!(r.hit_default);
        let e = &r.entities[0];
        assert_eq!(e.data["제목"], serde_json::json!("사용자 권한"));
        assert_eq!(e.data["태그"], serde_json::json!(["linux/시스템"]));
        assert_eq!(e.src_key, "path:Obsidian/Linux/권한.md");
    }

    #[test]
    fn wishlist_표는_행당_물건_카테고리는_경로유추() {
        let doc = parse_document("---\ntype: wishlist\n---\n\n| 제품 | 가격 | 링크 |\n| --- | --- | --- |\n| 세이코 미쿠 | 650,000원 | [m](<https://jp.mercari.com/item/m1>) |\n| 카시오 | 36,250원 |  |\n");
        let r = route_file(
            &rules(),
            &schemas(),
            "OneDrive/럭셔리/전자제품/모바일/모바일 위시리스트.md",
            &doc,
        );
        assert_eq!(r.entities.len(), 2);
        let a = &r.entities[0];
        assert_eq!(a.data["이름"], serde_json::json!("세이코 미쿠"));
        assert_eq!(
            a.data["가격"],
            serde_json::json!({ "amount": 650000.0, "currency": "KRW" })
        );
        assert_eq!(
            a.data["구매링크"],
            serde_json::json!("https://jp.mercari.com/item/m1")
        );
        assert_eq!(a.data["상태"], serde_json::json!("위시"));
        assert_eq!(a.data["카테고리"], serde_json::json!("전자제품"));
        assert_eq!(
            a.src_key,
            "row:OneDrive/럭셔리/전자제품/모바일/모바일 위시리스트.md#세이코 미쿠"
        );
    }

    #[test]
    fn 선택된_규칙의_스키마와_필드_오류는_부분_생성하지_않는다() {
        let cases = [
            (
                "unknown schema",
                "rules:\n  - match: { path: '*' }\n    to: 없음\n    map: { 제목: filename }\n  - default: { skip: true }\n",
            ),
            (
                "single unknown map",
                "rules:\n  - match: { path: '*' }\n    to: 노트\n    map: { 제목: filename, 선택필드오타: body }\n  - default: { skip: true }\n",
            ),
            (
                "table one unknown col",
                "rules:\n  - match: { path: '*' }\n    to: 물건\n    rows: table\n    cols: { 제품: 이름, 가격: 가격, 오타: 없는필드 }\n  - default: { skip: true }\n",
            ),
            (
                "table all unknown cols and set",
                "rules:\n  - match: { path: '*' }\n    to: 물건\n    rows: table\n    cols: { 제품: 없는필드, 가격: 또없는필드 }\n    set: { 설정오타: filename }\n  - default: { skip: true }\n",
            ),
            (
                "table unknown set",
                "rules:\n  - match: { path: '*' }\n    to: 물건\n    rows: table\n    cols: { 제품: 이름 }\n    set: { 설정오타: filename }\n  - default: { skip: true }\n",
            ),
        ];
        let doc = parse_document("| 제품 | 가격 |\n| --- | --- |\n| 카메라 | 60 USD |\n");

        for (name, yaml) in cases {
            let result = route_file(
                &RuleSet::from_yaml(yaml).unwrap(),
                &schemas(),
                "위시.md",
                &doc,
            );
            assert!(result.entities.is_empty(), "{name}");
            assert_eq!(result.dropped_required, 0, "{name}");
            assert!(
                !result.config_errors.is_empty()
                    && result
                        .config_errors
                        .iter()
                        .all(|error| error.starts_with("rules[0]:")),
                "{name}: {:?}",
                result.config_errors
            );
        }
    }

    #[test]
    fn 가격_파싱실패는_메모에_원문보존() {
        let doc = parse_document("---\ntype: wishlist\n---\n\n| 제품 | 가격 |\n| --- | --- |\n| 샤지 배터리 | 60 USD(외화) |\n");
        let r = route_file(
            &rules(),
            &schemas(),
            "OneDrive/전자제품/위시리스트.md",
            &doc,
        );
        assert_eq!(r.price_warnings, 1);
        let e = &r.entities[0];
        assert!(e.data.get("가격").is_none());
        assert_eq!(e.data["메모"], serde_json::json!("60 USD(외화)"));
        assert_eq!(
            e.data["$meta"]["메모"]["source"],
            serde_json::json!("imported")
        );
    }

    #[test]
    fn 가격_파싱실패는_기존_메모를_덮지_않고_뒤에_추가한다() {
        let doc = parse_document(
            "| 제품 | 가격 | 가격2 | 비고 |\n| --- | --- | --- | --- |\n| 배터리 | 60 USD | 70 EUR | 해외 배송 |\n",
        );
        let r = route_file(
            &rules(),
            &schemas(),
            "OneDrive/전자제품/위시리스트.md",
            &doc,
        );
        assert_eq!(r.price_warnings, 2);
        let e = &r.entities[0];
        assert_eq!(
            e.data["메모"],
            serde_json::json!("해외 배송\n60 USD\n70 EUR")
        );
        assert_eq!(
            e.data["$meta"]["메모"]["source"],
            serde_json::json!("imported")
        );
    }

    #[test]
    fn 메모를_저장할_수_없어도_가격_경고는_집계한다() {
        let price_rules = RuleSet::from_yaml(
            "rules:\n  - match: { path: '*' }\n    to: 물건\n    rows: table\n    cols: { 제품: 이름, 가격: 가격 }\n  - default: { skip: true }\n",
        )
        .unwrap();
        for memo_field in ["", "  메모: { kind: number }\n"] {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(
                dir.path().join("물건.yaml"),
                format!(
                    "type: 물건\nfields:\n  이름: {{ kind: text, required: true }}\n  가격: {{ kind: money }}\n{memo_field}"
                ),
            )
            .unwrap();
            let schemas = SchemaSet::load_dir(dir.path()).unwrap();
            let doc = parse_document("| 제품 | 가격 |\n| --- | --- |\n| 배터리 | 60 USD |\n");
            let r = route_file(&price_rules, &schemas, "위시리스트.md", &doc);
            assert_eq!(r.price_warnings, 1);
            assert!(r.entities[0].data.get("메모").is_none());
        }
    }

    #[test]
    fn 이름없는_행은_드롭() {
        let doc = parse_document(
            "---\ntype: wishlist\n---\n\n| 제품 | 가격 |\n| --- | --- |\n|  | 1,000원 |\n",
        );
        let r = route_file(
            &rules(),
            &schemas(),
            "OneDrive/시계/시계 위시리스트.md",
            &doc,
        );
        assert!(r.entities.is_empty());
        assert_eq!(r.dropped_required, 1);
    }

    #[test]
    fn moc는_스킵() {
        let r = route_file(
            &rules(),
            &schemas(),
            "x.md",
            &parse_document("---\ntype: moc\n---\n지도"),
        );
        assert!(r.skipped);
        assert!(r.entities.is_empty());
    }

    #[test]
    fn nfd_헤더와_경로는_nfc_필드와_enum에_매칭된다() {
        let doc = parse_document("| 제품 |\n| --- |\n| 카메라 |\n");
        let r = route_file(
            &rules(),
            &schemas(),
            "OneDrive/전자제품/위시리스트.md",
            &doc,
        );
        assert_eq!(r.entities[0].data["이름"], serde_json::json!("카메라"));
        assert_eq!(
            r.entities[0].data["카테고리"],
            serde_json::json!("전자제품")
        );
    }

    #[test]
    fn 중복_이름은_정규화된_안정키와_2부터_접미사를_쓴다() {
        let doc = parse_document(
            "| 제품 |\n| --- |\n|  카메라   가방  |\n| 카메라 가방 |\n| 카메라 가방 |\n",
        );
        let r = route_file(&rules(), &schemas(), "위시리스트.md", &doc);
        assert_eq!(
            r.entities[0].data["이름"],
            serde_json::json!("카메라   가방")
        );
        assert_eq!(r.entities[0].src_key, "row:위시리스트.md#카메라 가방");
        assert_eq!(r.entities[1].src_key, "row:위시리스트.md#카메라 가방-2");
        assert_eq!(r.entities[2].src_key, "row:위시리스트.md#카메라 가방-3");
    }
}
