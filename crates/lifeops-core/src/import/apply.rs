use crate::entity::store::EntityMutation;
use crate::entity::validate::{FieldError, ValidationError};
use crate::entity::EntityStore;
use crate::error::CoreError;
use crate::import::route::{route_file, validate_rules_schema, PlannedEntity};
use crate::import::rules::RuleSet;
use crate::import::ParsedDoc;
use crate::schema::SchemaSet;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub struct FileInput {
    pub relpath: String,
    pub doc: ParsedDoc,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PlanStats {
    pub scanned: usize,
    pub parse_warnings: usize,
    pub skipped_rule: usize,
    pub dropped_required: usize,
    pub default_fallback: usize,
    pub price_warnings: usize,
    pub routed: BTreeMap<String, usize>,
    pub config_errors: Vec<String>,
}

pub struct ImportPlan {
    pub entities: Vec<PlannedEntity>,
    pub stats: PlanStats,
}

pub struct ApplyOpts {
    pub force: bool,
}

#[derive(Debug, Default)]
pub struct ImportReport {
    pub stats: PlanStats,
    pub created: usize,
    pub updated: usize,
    pub skipped_existing: usize,
}

/// 파일들을 라우팅해 계획과 집계를 만든다. 규칙/schema 교차검증 실패 시 쓰기 후보는 만들지 않는다.
pub fn plan(rules: &RuleSet, schemas: &SchemaSet, files: &[FileInput]) -> ImportPlan {
    let mut stats = PlanStats {
        scanned: files.len(),
        parse_warnings: files.iter().filter(|file| !file.doc.frontmatter_ok).count(),
        config_errors: validate_rules_schema(rules, schemas),
        ..PlanStats::default()
    };
    if !stats.config_errors.is_empty() {
        return ImportPlan {
            entities: Vec::new(),
            stats,
        };
    }

    let mut entities = Vec::new();
    for file in files {
        let routed = route_file(rules, schemas, &file.relpath, &file.doc);
        extend_unique(&mut stats.config_errors, routed.config_errors);
        stats.dropped_required += routed.dropped_required;
        stats.price_warnings += routed.price_warnings;
        if routed.skipped {
            stats.skipped_rule += 1;
            continue;
        }
        if routed.hit_default {
            stats.default_fallback += 1;
        }
        for entity in routed.entities {
            *stats.routed.entry(entity.entity_type.clone()).or_default() += 1;
            entities.push(entity);
        }
    }
    if !stats.config_errors.is_empty() {
        entities.clear();
        stats.routed.clear();
    }
    ImportPlan { entities, stats }
}

/// 계획을 store에 반영한다. 같은 `$src`는 기본 스킵, force에서는 imported metadata를 유지해 갱신한다.
pub async fn apply(
    store: &EntityStore,
    schemas: &SchemaSet,
    plan: &ImportPlan,
    opts: ApplyOpts,
) -> Result<ImportReport, CoreError> {
    if !plan.stats.config_errors.is_empty() {
        return Err(validation_error(
            "$rules",
            plan.stats.config_errors.join(" / "),
        ));
    }

    let mut planned_sources = BTreeSet::new();
    for entity in &plan.entities {
        let persisted_src = match entity.data.get("$src") {
            Some(Value::String(src)) if !src.trim().is_empty() => src,
            Some(Value::String(_)) => {
                return Err(validation_error("$src", "빈 문자열일 수 없음".to_string()));
            }
            Some(_) => return Err(validation_error("$src", "문자열이어야 함".to_string())),
            None => return Err(validation_error("$src", "출처키가 필요함".to_string())),
        };
        if persisted_src != &entity.src_key {
            return Err(validation_error(
                "$src",
                format!(
                    "계획 출처키 '{}'와 저장 출처키 '{persisted_src}'가 일치해야 함",
                    entity.src_key
                ),
            ));
        }
        if !planned_sources.insert(persisted_src.clone()) {
            return Err(validation_error(
                "$src",
                format!("계획에 중복 출처키 '{persisted_src}': 쓰기를 중단함"),
            ));
        }
    }

    // `$src`는 타입을 가로지르는 식별자다. 다른 타입에 같은 키가 있는 경우도
    // create로 우회하지 않도록 전체 schema 타입에서 기존 키를 확인한다.
    let types: Vec<String> = schemas.names().into_iter().map(str::to_string).collect();
    let existing = store.list(&types).await?;
    let mut by_src = BTreeMap::<String, (String, String)>::new();
    for entity in existing {
        let Some(Value::String(src)) = entity.data.get("$src") else {
            continue;
        };
        if by_src
            .insert(src.clone(), (entity.id, entity.entity_type))
            .is_some()
        {
            return Err(validation_error(
                "$src",
                format!("저장소에 중복 출처키 '{src}': 쓰기를 중단함"),
            ));
        }
    }

    for planned in &plan.entities {
        if let Some((_, existing_type)) = by_src.get(&planned.src_key) {
            if existing_type != &planned.entity_type {
                return Err(validation_error(
                    "$src",
                    format!(
                        "출처키 '{}'는 기존 타입 '{}'인데 계획 타입 '{}'와 다름",
                        planned.src_key, existing_type, planned.entity_type
                    ),
                ));
            }
        }
    }

    let mut mutations = Vec::new();
    let mut created = 0;
    let mut updated = 0;
    let mut skipped_existing = 0;
    for planned in &plan.entities {
        if let Some((id, _)) = by_src.get(&planned.src_key) {
            if opts.force {
                let mut patch = planned.data.clone();
                patch.remove("$src");
                mutations.push(EntityMutation::Update {
                    id: id.clone(),
                    patch,
                });
                updated += 1;
            } else {
                skipped_existing += 1;
            }
        } else {
            mutations.push(EntityMutation::Create {
                entity_type: planned.entity_type.clone(),
                data: planned.data.clone(),
            });
            created += 1;
        }
    }
    store.apply_mutations_atomic(schemas, mutations).await?;
    Ok(ImportReport {
        stats: plan.stats.clone(),
        created,
        updated,
        skipped_existing,
    })
}

fn extend_unique(target: &mut Vec<String>, incoming: Vec<String>) {
    for error in incoming {
        if !target.contains(&error) {
            target.push(error);
        }
    }
}

fn validation_error(field: &str, message: String) -> CoreError {
    CoreError::Validation(ValidationError(vec![FieldError {
        field: field.to_string(),
        message,
    }]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::{parse_document, RuleSet};
    use crate::schema::SchemaSet;
    use serde_json::{json, Map};

    fn schemas() -> SchemaSet {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("노트.yaml"),
            "type: 노트\nfields:\n  제목: { kind: text, required: true }\n  본문: { kind: richtext }\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("기록.yaml"),
            "type: 기록\nfields:\n  제목: { kind: text, required: true }\n",
        )
        .unwrap();
        SchemaSet::load_dir(dir.path()).unwrap()
    }

    fn rules() -> RuleSet {
        RuleSet::from_yaml("rules:\n  - default:\n      to: 노트\n      map: { 제목: fm.title | filename, 본문: body }\n      provenance: imported\n").unwrap()
    }

    fn file(relpath: &str, content: &str) -> FileInput {
        FileInput {
            relpath: relpath.to_string(),
            doc: parse_document(content),
        }
    }

    #[test]
    fn plan은_라우팅_집계를_낸다() {
        let files = vec![
            file("a.md", "---\ntitle: A\n---\n본문A"),
            file("b.md", "---\ntitle: B\n---\n본문B"),
        ];
        let planned = plan(&rules(), &schemas(), &files);
        assert_eq!(planned.entities.len(), 2);
        assert_eq!(planned.stats.scanned, 2);
        assert_eq!(planned.stats.default_fallback, 2);
        assert_eq!(planned.stats.routed["노트"], 2);
    }

    #[test]
    fn 규칙_schema_오류는_스캔과_파싱경고만_집계하고_라우팅하지_않는다() {
        let rules =
            RuleSet::from_yaml("rules:\n  - default:\n      to: 노트\n      map: { 오타: body }\n")
                .unwrap();
        let files = vec![file("a.md", "본문")];
        let planned = plan(&rules, &schemas(), &files);
        assert_eq!(planned.stats.scanned, 1);
        assert_eq!(planned.stats.parse_warnings, 1);
        assert!(!planned.stats.config_errors.is_empty());
        assert!(planned.stats.routed.is_empty());
        assert!(planned.entities.is_empty());
    }

    #[tokio::test]
    async fn 최초커밋은_생성_재실행은_스킵_force는_출처메타를_유지해_갱신() {
        let schemas = schemas();
        let store = EntityStore::open_in_memory().await.unwrap();
        let first = plan(
            &rules(),
            &schemas,
            &[file("a.md", "---\ntitle: 원제목\n---\n본문")],
        );
        let r1 = apply(&store, &schemas, &first, ApplyOpts { force: false })
            .await
            .unwrap();
        assert_eq!((r1.created, r1.updated, r1.skipped_existing), (1, 0, 0));
        assert_eq!(r1.stats, first.stats);

        let r2 = apply(&store, &schemas, &first, ApplyOpts { force: false })
            .await
            .unwrap();
        assert_eq!((r2.created, r2.updated, r2.skipped_existing), (0, 0, 1));

        let changed = plan(
            &rules(),
            &schemas,
            &[file("a.md", "---\ntitle: 새제목\n---\n본문")],
        );
        let r3 = apply(&store, &schemas, &changed, ApplyOpts { force: true })
            .await
            .unwrap();
        assert_eq!((r3.created, r3.updated, r3.skipped_existing), (0, 1, 0));
        let all = store.list(&["노트".to_string()]).await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].data["제목"], json!("새제목"));
        assert_eq!(all[0].data["$src"], json!("path:a.md"));
        assert_eq!(all[0].data["$meta"]["제목"]["source"], json!("imported"));
        assert!(all[0].data["$meta"].get("$src").is_none());
    }

    #[tokio::test]
    async fn config_error_plan은_apply가_조회나_쓰기_전에_거부한다() {
        let schemas = schemas();
        let store = EntityStore::open_in_memory().await.unwrap();
        let planned = ImportPlan {
            entities: vec![],
            stats: PlanStats {
                config_errors: vec!["bad config".to_string()],
                ..PlanStats::default()
            },
        };
        let error = apply(&store, &schemas, &planned, ApplyOpts { force: false })
            .await
            .unwrap_err();
        assert!(matches!(error, CoreError::Validation(_)));
        assert!(store.list(&["노트".to_string()]).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn 계획과_저장소의_중복_src는_부분쓰기_전에_거부한다() {
        let schemas = schemas();
        let store = EntityStore::open_in_memory().await.unwrap();
        let mut planned = plan(
            &rules(),
            &schemas,
            &[
                file("a.md", "---\ntitle: A\n---\n"),
                file("a.md", "---\ntitle: B\n---\n"),
            ],
        );
        let error = apply(&store, &schemas, &planned, ApplyOpts { force: false })
            .await
            .unwrap_err();
        assert!(matches!(error, CoreError::Validation(_)));
        assert!(store.list(&["노트".to_string()]).await.unwrap().is_empty());

        planned.entities.truncate(1);
        for title in ["기존1", "기존2"] {
            let mut data = Map::new();
            data.insert("제목".to_string(), json!(title));
            data.insert("$src".to_string(), json!("path:a.md"));
            store.create(&schemas, "노트", data).await.unwrap();
        }
        let error = apply(&store, &schemas, &planned, ApplyOpts { force: true })
            .await
            .unwrap_err();
        match error {
            CoreError::Validation(error) => assert_eq!(error.0[0].field, "$src"),
            other => panic!("unexpected error: {other}"),
        }
        let all = store.list(&["노트".to_string()]).await.unwrap();
        assert!(all.iter().all(|entity| entity.data["제목"] != json!("A")));
    }

    #[tokio::test]
    async fn data_src가_없거나_문자열이_아니거나_src_key와_다르면_쓰기_전에_거부한다() {
        let schemas = schemas();
        for (name, stored_src) in [
            ("missing", None),
            ("non-string", Some(json!(42))),
            ("empty", Some(json!("  "))),
            ("mismatch", Some(json!("path:other.md"))),
        ] {
            let store = EntityStore::open_in_memory().await.unwrap();
            let mut data = Map::new();
            data.insert("제목".to_string(), json!(name));
            if let Some(stored_src) = stored_src {
                data.insert("$src".to_string(), stored_src);
            }
            let planned = ImportPlan {
                entities: vec![PlannedEntity {
                    entity_type: "노트".to_string(),
                    src_key: "path:a.md".to_string(),
                    data,
                }],
                stats: PlanStats::default(),
            };
            let error = apply(&store, &schemas, &planned, ApplyOpts { force: false })
                .await
                .unwrap_err();
            match error {
                CoreError::Validation(error) => assert_eq!(error.0[0].field, "$src", "{name}"),
                other => panic!("{name}: unexpected error: {other}"),
            }
            assert!(
                store.list(&["노트".to_string()]).await.unwrap().is_empty(),
                "{name}"
            );
        }
    }

    #[tokio::test]
    async fn 서로_다른_src_key로_같은_data_src를_숨겨도_쓰기_전에_거부한다() {
        let schemas = schemas();
        let store = EntityStore::open_in_memory().await.unwrap();
        let entities = ["path:a.md", "path:b.md"]
            .into_iter()
            .map(|src_key| {
                let mut data = Map::new();
                data.insert("제목".to_string(), json!(src_key));
                data.insert("$src".to_string(), json!("path:a.md"));
                PlannedEntity {
                    entity_type: "노트".to_string(),
                    src_key: src_key.to_string(),
                    data,
                }
            })
            .collect();
        let planned = ImportPlan {
            entities,
            stats: PlanStats::default(),
        };
        let error = apply(&store, &schemas, &planned, ApplyOpts { force: false })
            .await
            .unwrap_err();
        assert!(matches!(error, CoreError::Validation(_)));
        assert!(store.list(&["노트".to_string()]).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn batch_중간_실패는_앞선_생성도_rollback한다() {
        let schemas = schemas();
        let store = EntityStore::open_in_memory().await.unwrap();
        let make = |entity_type: &str, src: &str| {
            let mut data = Map::new();
            data.insert("제목".to_string(), json!(src));
            data.insert("$src".to_string(), json!(src));
            PlannedEntity {
                entity_type: entity_type.to_string(),
                src_key: src.to_string(),
                data,
            }
        };
        let planned = ImportPlan {
            entities: vec![
                make("노트", "path:valid.md"),
                make("없는타입", "path:invalid.md"),
            ],
            stats: PlanStats::default(),
        };

        let error = apply(&store, &schemas, &planned, ApplyOpts { force: false })
            .await
            .unwrap_err();
        assert!(matches!(error, CoreError::UnknownType(ref ty) if ty == "없는타입"));
        assert!(store.list(&["노트".to_string()]).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn 같은_src의_기존_타입과_계획_타입이_다르면_default와_force_모두_쓰기_전_거부한다() {
        let schemas = schemas();
        for force in [false, true] {
            let store = EntityStore::open_in_memory().await.unwrap();
            let mut existing = Map::new();
            existing.insert("제목".to_string(), json!("기존"));
            existing.insert("$src".to_string(), json!("path:a.md"));
            store.create(&schemas, "노트", existing).await.unwrap();

            let mut data = Map::new();
            data.insert("제목".to_string(), json!("새 기록"));
            data.insert("$src".to_string(), json!("path:a.md"));
            let planned = ImportPlan {
                entities: vec![PlannedEntity {
                    entity_type: "기록".to_string(),
                    src_key: "path:a.md".to_string(),
                    data,
                }],
                stats: PlanStats::default(),
            };
            let error = apply(&store, &schemas, &planned, ApplyOpts { force })
                .await
                .unwrap_err();
            match error {
                CoreError::Validation(error) => assert_eq!(error.0[0].field, "$src"),
                other => panic!("unexpected error: {other}"),
            }
            assert_eq!(store.list(&["노트".to_string()]).await.unwrap().len(), 1);
            assert!(store.list(&["기록".to_string()]).await.unwrap().is_empty());
        }
    }
}
