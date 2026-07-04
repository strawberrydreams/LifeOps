use lifeops_core::entity::EntityStore;
use lifeops_core::schema::{CategoryDef, SchemaSet};
use lifeops_core::view::PageSet;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
#[allow(dead_code)]
pub struct AppState {
    pub schemas: Arc<RwLock<SchemaSet>>,
    pub pages: Arc<RwLock<PageSet>>,
    pub categories: Arc<RwLock<Vec<CategoryDef>>>,
    pub store: Arc<EntityStore>,
    pub schemas_dir: PathBuf,
    pub views_dir: PathBuf,
    pub categories_path: PathBuf,
}

impl AppState {
    pub fn new(
        schemas: SchemaSet,
        pages: PageSet,
        categories: Vec<CategoryDef>,
        store: EntityStore,
        schemas_dir: PathBuf,
        views_dir: PathBuf,
        categories_path: PathBuf,
    ) -> Self {
        AppState {
            schemas: Arc::new(RwLock::new(schemas)),
            pages: Arc::new(RwLock::new(pages)),
            categories: Arc::new(RwLock::new(categories)),
            store: Arc::new(store),
            schemas_dir,
            views_dir,
            categories_path,
        }
    }
}

#[cfg(test)]
pub async fn test_state() -> (AppState, tempfile::TempDir) {
    use lifeops_core::schema::SchemaSet;
    use lifeops_core::view::PageSet;
    let dir = tempfile::tempdir().unwrap();
    let sdir = dir.path().join("schemas");
    let vdir = dir.path().join("views");
    std::fs::create_dir_all(&sdir).unwrap();
    std::fs::create_dir_all(&vdir).unwrap();
    std::fs::write(
        sdir.join("물건.yaml"),
        "type: 물건\ncategory: 컬렉션\nfields:\n  이름: { kind: text, required: true }\n  상태: { kind: enum, options: [위시, 주문됨, 보유, 과거] }\n  가격: { kind: money }\n",
    )
    .unwrap();
    std::fs::write(
        sdir.join("시계.yaml"),
        "type: 시계\nextends: 물건\nfields:\n  무브먼트: { kind: text }\n",
    )
    .unwrap();
    std::fs::write(
        sdir.join("할일.yaml"),
        "type: 할일\nbehaviors:\n  recurrence: { flag: 완료, rule: 반복, date: 마감일 }\nfields:\n  내용: { kind: text, required: true }\n  완료: { kind: bool }\n  마감일: { kind: date }\n  반복: { kind: text }\n  관련물건: { kind: \"list<ref>\", target: 물건 }\n",
    )
    .unwrap();
    let cat_path = dir.path().join("categories.yaml");
    std::fs::write(
        &cat_path,
        "categories:\n  - { name: 할일, icon: \"✅\" }\n  - { name: 컬렉션, icon: \"📦\" }\n",
    )
    .unwrap();
    let categories = lifeops_core::schema::load_categories(&cat_path).unwrap();
    let schemas = SchemaSet::load_dir(&sdir).unwrap();
    let pages = PageSet::load_dir(&vdir).unwrap();
    let store = lifeops_core::entity::EntityStore::open_in_memory()
        .await
        .unwrap();
    (
        AppState::new(schemas, pages, categories, store, sdir, vdir, cat_path),
        dir,
    )
}
