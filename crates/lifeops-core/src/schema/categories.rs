use crate::error::SchemaError;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct CategoryDef {
    pub name: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CategoriesFile {
    categories: Vec<CategoryDef>,
}

pub fn load_categories(path: &Path) -> Result<Vec<CategoryDef>, SchemaError> {
    if !path.exists() {
        return Ok(Vec::new());
    }

    let text = std::fs::read_to_string(path)?;
    let file: CategoriesFile =
        serde_yaml::from_str(&text).map_err(|source| SchemaError::Parse {
            file: path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_default(),
            source,
        })?;
    Ok(file.categories)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 카테고리_로드_순서_보존() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("categories.yaml");
        std::fs::write(
            &p,
            "categories:\n  - { name: 할일, icon: \"✅\" }\n  - { name: 메모 }\n",
        )
        .unwrap();
        let cats = load_categories(&p).unwrap();
        assert_eq!(cats.len(), 2);
        assert_eq!(cats[0].name, "할일");
        assert_eq!(cats[0].icon.as_deref(), Some("✅"));
        assert!(cats[1].icon.is_none());
    }

    #[test]
    fn 파일_없으면_빈_목록() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_categories(&dir.path().join("없음.yaml"))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn 깨진_yaml은_에러() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("categories.yaml");
        std::fs::write(&p, "categories: [broken").unwrap();
        assert!(load_categories(&p).is_err());
    }
}
