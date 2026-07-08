use crate::error::SchemaError;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RawSchema {
    #[serde(rename = "type")]
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extends: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub singleton: bool,
    #[serde(default, skip_serializing_if = "is_empty_behaviors")]
    pub behaviors: Option<RawBehaviors>,
    #[serde(default)]
    pub fields: IndexMap<String, RawFieldDef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field_order: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct RawBehaviors {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recurrence: Option<RecurrenceDef>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct RecurrenceDef {
    pub flag: String,
    pub rule: String,
    pub date: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RawFieldDef {
    pub kind: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_empty_behaviors(behaviors: &Option<RawBehaviors>) -> bool {
    match behaviors {
        None => true,
        Some(behaviors) => behaviors.recurrence.is_none(),
    }
}

pub fn load_raw_dir(dir: &Path) -> Result<IndexMap<String, (RawSchema, String)>, SchemaError> {
    let mut files: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "yaml" || x == "yml"))
        .collect();
    files.sort();

    let mut out: IndexMap<String, (RawSchema, String)> = IndexMap::new();
    for path in files {
        let file = path.file_name().unwrap().to_string_lossy().to_string();
        let text = std::fs::read_to_string(&path)?;
        let schema: RawSchema =
            serde_yaml::from_str(&text).map_err(|source| SchemaError::Parse {
                file: file.clone(),
                source,
            })?;
        if let Some((first_schema, first_file)) = out.get(&schema.name) {
            let _ = first_schema;
            return Err(SchemaError::DuplicateType {
                file,
                name: schema.name.clone(),
                first: first_file.clone(),
            });
        }
        out.insert(schema.name.clone(), (schema, file));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &std::path::Path, name: &str, body: &str) {
        std::fs::write(dir.join(name), body).unwrap();
    }

    #[test]
    fn 디렉터리에서_스키마를_읽고_필드_순서를_보존한다() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "물건.yaml",
            "type: 물건\nfields:\n  이름: { kind: text, required: true }\n  가격: { kind: money }\n  상태: { kind: enum, options: [위시, 보유] }\n",
        );
        let raw = load_raw_dir(dir.path()).unwrap();
        let (schema, file) = &raw["물건"];
        assert_eq!(schema.name, "물건");
        assert_eq!(file, "물건.yaml");
        let names: Vec<&str> = schema.fields.keys().map(|s| s.as_str()).collect();
        assert_eq!(names, vec!["이름", "가격", "상태"]); // 선언 순서 보존
        assert!(schema.fields["이름"].required);
        assert_eq!(
            schema.fields["상태"].options.as_deref(),
            Some(&["위시".to_string(), "보유".to_string()][..])
        );
    }

    #[test]
    fn 중복_타입명은_에러() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "a.yaml", "type: 물건\nfields: {}\n");
        write(dir.path(), "b.yaml", "type: 물건\nfields: {}\n");
        let err = load_raw_dir(dir.path()).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("물건") && msg.contains("b.yaml"),
            "메시지: {msg}"
        );
    }

    #[test]
    fn 깨진_yaml은_파일명을_포함한_에러() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "bad.yaml", "type: [broken");
        let err = load_raw_dir(dir.path()).unwrap_err();
        assert!(err.to_string().contains("bad.yaml"));
    }

    #[test]
    fn singleton_플래그를_읽고_기본은_false() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "프로필.yaml",
            "type: 프로필\nsingleton: true\nfields:\n  이름: { kind: text }\n",
        );
        write(
            dir.path(),
            "물건.yaml",
            "type: 물건\nfields:\n  이름: { kind: text }\n",
        );
        let raw = load_raw_dir(dir.path()).unwrap();
        assert!(raw["프로필"].0.singleton, "프로필은 singleton=true");
        assert!(!raw["물건"].0.singleton, "물건은 기본 false");
    }
}
