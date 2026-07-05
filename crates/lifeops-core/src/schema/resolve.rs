use crate::error::SchemaError;
use crate::schema::raw::{load_raw_dir, RawBehaviors, RawFieldDef, RawSchema};
use crate::schema::FieldKind;
use indexmap::IndexMap;
use std::path::Path;

#[derive(Debug, Clone, serde::Serialize)]
pub struct ResolvedField {
    pub kind: FieldKind,
    pub required: bool,
    pub options: Option<Vec<String>>,
    pub target: Option<String>,
    pub unit: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ResolvedSchema {
    pub name: String,
    pub extends: Option<String>,
    pub category: Option<String>,
    pub behaviors: Option<RawBehaviors>,
    pub fields: IndexMap<String, ResolvedField>,
}

#[derive(Debug)]
pub struct SchemaSet {
    schemas: IndexMap<String, ResolvedSchema>,
    children: IndexMap<String, Vec<String>>,
}

impl SchemaSet {
    pub fn load_dir(dir: &Path) -> Result<SchemaSet, SchemaError> {
        let raw = load_raw_dir(dir)?;
        Self::from_raw(&raw)
    }

    pub fn from_raw(raw: &IndexMap<String, (RawSchema, String)>) -> Result<SchemaSet, SchemaError> {
        let mut schemas = IndexMap::new();
        for name in raw.keys() {
            let chain = ancestor_chain(raw, name)?; // [루트, ..., 자기 자신]
            let mut merged: IndexMap<String, RawFieldDef> = IndexMap::new();
            let mut category: Option<String> = None;
            let mut behaviors: Option<RawBehaviors> = None;
            for ty in &chain {
                let (schema, _) = &raw[ty];
                if schema.category.is_some() {
                    category = schema.category.clone();
                }
                if schema.behaviors.is_some() {
                    behaviors = schema.behaviors.clone();
                }
                for (fname, def) in &schema.fields {
                    // 재선언은 부모 위치를 유지한 채 정의만 교체
                    merged.insert(fname.clone(), def.clone());
                }
            }
            let (leaf, _) = &raw[name];
            if let Some(order) = &leaf.field_order {
                let mut reordered = IndexMap::new();
                for fname in order {
                    match merged.shift_remove(fname) {
                        Some(def) => {
                            reordered.insert(fname.clone(), def);
                        }
                        None => {
                            return Err(SchemaError::UnknownFieldInOrder {
                                ty: name.clone(),
                                field: fname.clone(),
                            })
                        }
                    }
                }
                reordered.extend(merged); // 누락 필드는 기본 순서대로 뒤에
                merged = reordered;
            }
            let mut fields = IndexMap::new();
            for (fname, def) in merged {
                let field = convert_field(name, &fname, &def)?;
                validate_ref_target(raw, name, &fname, &field)?;
                fields.insert(fname.clone(), field);
            }
            if let Some(behaviors) = &behaviors {
                validate_behaviors(name, behaviors, &fields)?;
            }
            schemas.insert(
                name.clone(),
                ResolvedSchema {
                    name: name.clone(),
                    extends: leaf.extends.clone(),
                    category,
                    behaviors,
                    fields,
                },
            );
        }
        let mut children: IndexMap<String, Vec<String>> = IndexMap::new();
        for (name, (schema, _)) in raw {
            if let Some(parent) = &schema.extends {
                children
                    .entry(parent.clone())
                    .or_default()
                    .push(name.clone());
            }
        }
        Ok(SchemaSet { schemas, children })
    }

    pub fn get(&self, name: &str) -> Option<&ResolvedSchema> {
        self.schemas.get(name)
    }

    pub fn names(&self) -> Vec<&str> {
        self.schemas.keys().map(|s| s.as_str()).collect()
    }

    /// 자신 + 모든 자손 타입명 (뷰의 source 확장에 사용)
    pub fn family_of(&self, name: &str) -> Vec<String> {
        let mut out = vec![name.to_string()];
        let mut queue = vec![name.to_string()];
        while let Some(cur) = queue.pop() {
            if let Some(kids) = self.children.get(&cur) {
                for k in kids {
                    out.push(k.clone());
                    queue.push(k.clone());
                }
            }
        }
        out
    }
}

/// 루트 조상부터 자기 자신까지의 체인. 순환·없는 부모 감지.
fn ancestor_chain(
    raw: &IndexMap<String, (RawSchema, String)>,
    name: &str,
) -> Result<Vec<String>, SchemaError> {
    let mut chain = vec![name.to_string()];
    let mut cur = name.to_string();
    while let Some(parent) = raw[&cur].0.extends.clone() {
        if !raw.contains_key(&parent) {
            return Err(SchemaError::UnknownParent { ty: cur, parent });
        }
        if chain.contains(&parent) {
            chain.push(parent);
            return Err(SchemaError::Cycle {
                chain: chain.join(" → "),
            });
        }
        chain.push(parent.clone());
        cur = parent;
    }
    chain.reverse();
    Ok(chain)
}

fn convert_field(ty: &str, field: &str, def: &RawFieldDef) -> Result<ResolvedField, SchemaError> {
    let kind = FieldKind::parse(&def.kind).ok_or_else(|| SchemaError::BadKind {
        ty: ty.to_string(),
        field: field.to_string(),
        value: def.kind.clone(),
    })?;
    let is_enum = matches!(kind, FieldKind::Enum)
        || matches!(&kind, FieldKind::List(i) if **i == FieldKind::Enum);
    if is_enum && def.options.is_none() {
        return Err(SchemaError::EnumWithoutOptions {
            ty: ty.to_string(),
            field: field.to_string(),
        });
    }
    Ok(ResolvedField {
        kind,
        required: def.required,
        options: def.options.clone(),
        target: def.target.clone(),
        unit: def.unit.clone(),
    })
}

fn validate_ref_target(
    raw: &IndexMap<String, (RawSchema, String)>,
    ty: &str,
    field: &str,
    def: &ResolvedField,
) -> Result<(), SchemaError> {
    let is_ref = matches!(def.kind, FieldKind::Ref)
        || matches!(&def.kind, FieldKind::List(inner) if **inner == FieldKind::Ref);
    let Some(target) = &def.target else {
        return Ok(());
    };
    if is_ref && !raw.contains_key(target) {
        return Err(SchemaError::UnknownRefTarget {
            ty: ty.to_string(),
            field: field.to_string(),
            target: target.clone(),
        });
    }
    Ok(())
}

fn validate_behaviors(
    ty: &str,
    behaviors: &RawBehaviors,
    fields: &IndexMap<String, ResolvedField>,
) -> Result<(), SchemaError> {
    if let Some(recurrence) = &behaviors.recurrence {
        validate_recurrence_field(ty, "flag", &recurrence.flag, FieldKind::Bool, fields)?;
        validate_recurrence_field(ty, "rule", &recurrence.rule, FieldKind::Text, fields)?;
        validate_recurrence_field(ty, "date", &recurrence.date, FieldKind::Date, fields)?;
    }
    Ok(())
}

fn validate_recurrence_field(
    ty: &str,
    role: &str,
    field: &str,
    expected: FieldKind,
    fields: &IndexMap<String, ResolvedField>,
) -> Result<(), SchemaError> {
    let Some(resolved) = fields.get(field) else {
        return Err(SchemaError::BadBehavior {
            ty: ty.to_string(),
            message: format!("recurrence.{role} 필드 '{field}'를 찾을 수 없음"),
        });
    };
    if resolved.kind != expected {
        return Err(SchemaError::BadBehavior {
            ty: ty.to_string(),
            message: format!(
                "recurrence.{role} 필드 '{field}' kind는 {expected}이어야 함 (현재 {})",
                resolved.kind
            ),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::FieldKind;

    fn set_from(files: &[(&str, &str)]) -> Result<SchemaSet, crate::error::SchemaError> {
        let dir = tempfile::tempdir().unwrap();
        for (name, body) in files {
            std::fs::write(dir.path().join(name), body).unwrap();
        }
        SchemaSet::load_dir(dir.path())
    }

    const 물건: &str = "type: 물건\nfields:\n  이름: { kind: text, required: true }\n  가격: { kind: money }\n  상태: { kind: enum, options: [위시, 보유] }\n";

    #[test]
    fn category와_behaviors는_상속되고_리프가_재정의한다() {
        let set = set_from(&[
            (
                "물건.yaml",
                "type: 물건\ncategory: 컬렉션\nbehaviors:\n  recurrence: { flag: 완료, rule: 반복, date: 마감일 }\nfields:\n  이름: { kind: text, required: true }\n  완료: { kind: bool }\n  반복: { kind: text }\n  마감일: { kind: date }\n",
            ),
            ("시계.yaml", "type: 시계\nextends: 물건\nfields: {}\n"),
            (
                "잡화.yaml",
                "type: 잡화\nextends: 물건\ncategory: 기타상자\nbehaviors:\n  recurrence: { flag: 구입, rule: 주기, date: 예정일 }\nfields:\n  구입: { kind: bool }\n  주기: { kind: text }\n  예정일: { kind: date }\n",
            ),
        ])
        .unwrap();
        let parent = set.get("물건").unwrap();
        assert_eq!(parent.category.as_deref(), Some("컬렉션"));
        let parent_rec = parent
            .behaviors
            .as_ref()
            .unwrap()
            .recurrence
            .as_ref()
            .unwrap();
        assert_eq!(
            (
                parent_rec.flag.as_str(),
                parent_rec.rule.as_str(),
                parent_rec.date.as_str()
            ),
            ("완료", "반복", "마감일")
        );

        let child = set.get("시계").unwrap();
        assert_eq!(child.category.as_deref(), Some("컬렉션"));
        let child_rec = child
            .behaviors
            .as_ref()
            .unwrap()
            .recurrence
            .as_ref()
            .unwrap();
        assert_eq!(
            (
                child_rec.flag.as_str(),
                child_rec.rule.as_str(),
                child_rec.date.as_str()
            ),
            ("완료", "반복", "마감일")
        );

        let override_child = set.get("잡화").unwrap();
        assert_eq!(override_child.category.as_deref(), Some("기타상자"));
        let override_rec = override_child
            .behaviors
            .as_ref()
            .unwrap()
            .recurrence
            .as_ref()
            .unwrap();
        assert_eq!(
            (
                override_rec.flag.as_str(),
                override_rec.rule.as_str(),
                override_rec.date.as_str()
            ),
            ("구입", "주기", "예정일")
        );
    }

    #[test]
    fn 미지정_category는_none() {
        let set = set_from(&[("물건.yaml", 물건)]).unwrap();
        assert!(set.get("물건").unwrap().category.is_none());
    }

    #[test]
    fn behaviors_recurrence_필드_kind_검증() {
        let ok = "type: 할일\nbehaviors:\n  recurrence: { flag: 완료, rule: 반복, date: 마감일 }\nfields:\n  완료: { kind: bool }\n  반복: { kind: text }\n  마감일: { kind: date }\n";
        let set = set_from(&[("할일.yaml", ok)]).unwrap();
        let rec = set
            .get("할일")
            .unwrap()
            .behaviors
            .as_ref()
            .unwrap()
            .recurrence
            .as_ref()
            .unwrap();
        assert_eq!(
            (rec.flag.as_str(), rec.rule.as_str(), rec.date.as_str()),
            ("완료", "반복", "마감일")
        );

        let bad = "type: 할일\nbehaviors:\n  recurrence: { flag: 마감일, rule: 반복, date: 마감일 }\nfields:\n  반복: { kind: text }\n  마감일: { kind: date }\n";
        let err = set_from(&[("할일.yaml", bad)]).unwrap_err();
        match err {
            crate::error::SchemaError::BadBehavior { ty, message } => {
                assert_eq!(ty, "할일");
                assert!(message.contains("recurrence"));
                assert!(message.contains("마감일"));
            }
            other => panic!("BadBehavior를 기대했지만 {other:?}"),
        }

        let missing = "type: 할일\nbehaviors:\n  recurrence: { flag: 유령, rule: 반복, date: 마감일 }\nfields:\n  반복: { kind: text }\n  마감일: { kind: date }\n";
        assert!(set_from(&[("할일.yaml", missing)]).is_err());
    }

    #[test]
    fn 상속_병합_순서는_부모_먼저() {
        let set = set_from(&[
            ("물건.yaml", 물건),
            (
                "시계.yaml",
                "type: 시계\nextends: 물건\nfields:\n  무브먼트: { kind: text }\n",
            ),
        ])
        .unwrap();
        let watch = set.get("시계").unwrap();
        let names: Vec<&str> = watch.fields.keys().map(|s| s.as_str()).collect();
        assert_eq!(names, vec!["이름", "가격", "상태", "무브먼트"]);
        assert!(watch.fields["이름"].required); // 부모 속성 유지
    }

    #[test]
    fn field_order로_재배치하고_누락_필드는_뒤에_붙는다() {
        let set = set_from(&[
            ("물건.yaml", 물건),
            ("시계.yaml", "type: 시계\nextends: 물건\nfields:\n  무브먼트: { kind: text }\nfield_order: [무브먼트, 이름]\n"),
        ])
        .unwrap();
        let names: Vec<&str> = set
            .get("시계")
            .unwrap()
            .fields
            .keys()
            .map(|s| s.as_str())
            .collect();
        assert_eq!(names, vec!["무브먼트", "이름", "가격", "상태"]);
    }

    #[test]
    fn field_order의_오타는_에러() {
        let err = set_from(&[
            ("물건.yaml", 물건),
            (
                "시계.yaml",
                "type: 시계\nextends: 물건\nfields: {}\nfield_order: [없는필드]\n",
            ),
        ])
        .unwrap_err();
        assert!(err.to_string().contains("없는필드"));
    }

    #[test]
    fn 자식이_같은_필드를_재선언하면_덮어쓴다() {
        let set = set_from(&[
            ("물건.yaml", 물건),
            ("시계.yaml", "type: 시계\nextends: 물건\nfields:\n  상태: { kind: enum, options: [위시, 주문됨, 보유, 과거] }\n"),
        ])
        .unwrap();
        let f = &set.get("시계").unwrap().fields["상태"];
        assert_eq!(f.options.as_ref().unwrap().len(), 4);
        // 위치는 부모 자리 유지
        let names: Vec<&str> = set
            .get("시계")
            .unwrap()
            .fields
            .keys()
            .map(|s| s.as_str())
            .collect();
        assert_eq!(names, vec!["이름", "가격", "상태"]);
    }

    #[test]
    fn 다단계_상속과_family_of() {
        let set = set_from(&[
            ("물건.yaml", 물건),
            ("시계.yaml", "type: 시계\nextends: 물건\nfields: {}\n"),
            (
                "스마트워치.yaml",
                "type: 스마트워치\nextends: 시계\nfields:\n  os: { kind: text }\n",
            ),
        ])
        .unwrap();
        assert!(set.get("스마트워치").unwrap().fields.contains_key("이름"));
        let mut fam = set.family_of("물건");
        fam.sort();
        assert_eq!(fam, vec!["물건", "스마트워치", "시계"]);
        assert_eq!(set.family_of("스마트워치"), vec!["스마트워치"]);
    }

    #[test]
    fn 순환_상속은_에러() {
        let err = set_from(&[
            ("a.yaml", "type: A\nextends: B\nfields: {}\n"),
            ("b.yaml", "type: B\nextends: A\nfields: {}\n"),
        ])
        .unwrap_err();
        assert!(err.to_string().contains("순환"));
    }

    #[test]
    fn 없는_부모는_에러() {
        let err = set_from(&[("a.yaml", "type: A\nextends: 유령\nfields: {}\n")]).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("A") && msg.contains("유령"));
    }

    #[test]
    fn target_없는_ref는_허용된다() {
        let set = set_from(&[(
            "할일.yaml",
            "type: 할일\nfields:\n  내용: { kind: text, required: true }\n  관련: { kind: \"list<ref>\" }\n",
        )])
        .unwrap();
        let f = &set.get("할일").unwrap().fields["관련"];
        assert_eq!(f.kind, FieldKind::List(Box::new(FieldKind::Ref)));
        assert!(f.target.is_none());
    }

    #[test]
    fn 명시한_ref_target이_없으면_에러() {
        let err = set_from(&[(
            "할일.yaml",
            "type: 할일\nfields:\n  관련: { kind: ref, target: 유령 }\n",
        )])
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("할일"));
        assert!(msg.contains("관련"));
        assert!(msg.contains("유령"));
    }

    #[test]
    fn 잘못된_kind와_제약_누락은_에러() {
        let err = set_from(&[("a.yaml", "type: A\nfields:\n  x: { kind: geo }\n")]).unwrap_err();
        assert!(err.to_string().contains("geo"));
        let err = set_from(&[("a.yaml", "type: A\nfields:\n  x: { kind: enum }\n")]).unwrap_err();
        assert!(err.to_string().contains("options"));
        let set = set_from(&[
            ("물건.yaml", 물건),
            (
                "a.yaml",
                "type: A\nfields:\n  x: { kind: \"list<ref>\", target: 물건 }\n",
            ),
        ])
        .unwrap();
        assert_eq!(
            set.get("A").unwrap().fields["x"].kind,
            FieldKind::List(Box::new(FieldKind::Ref))
        );
    }
}
