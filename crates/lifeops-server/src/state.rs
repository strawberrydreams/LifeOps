use lifeops_core::entity::EntityStore;
use lifeops_core::schema::SchemaSet;
use lifeops_core::view::PageSet;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
#[allow(dead_code)]
pub struct AppState {
    pub schemas: Arc<RwLock<SchemaSet>>,
    pub pages: Arc<RwLock<PageSet>>,
    pub store: Arc<EntityStore>,
    pub schemas_dir: PathBuf,
    pub views_dir: PathBuf,
}

impl AppState {
    pub fn new(
        schemas: SchemaSet,
        pages: PageSet,
        store: EntityStore,
        schemas_dir: PathBuf,
        views_dir: PathBuf,
    ) -> Self {
        AppState {
            schemas: Arc::new(RwLock::new(schemas)),
            pages: Arc::new(RwLock::new(pages)),
            store: Arc::new(store),
            schemas_dir,
            views_dir,
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
        "type: 물건\nfields:\n  이름: { kind: text, required: true }\n  상태: { kind: enum, options: [위시, 주문됨, 보유, 과거] }\n  가격: { kind: money }\n",
    )
    .unwrap();
    std::fs::write(
        sdir.join("시계.yaml"),
        "type: 시계\nextends: 물건\nfields:\n  무브먼트: { kind: text }\n",
    )
    .unwrap();
    std::fs::write(
        sdir.join("할일.yaml"),
        "type: 할일\nfields:\n  내용: { kind: text, required: true }\n  완료: { kind: bool }\n  관련물건: { kind: \"list<ref>\", target: 물건 }\n",
    )
    .unwrap();
    let schemas = SchemaSet::load_dir(&sdir).unwrap();
    let pages = PageSet::load_dir(&vdir).unwrap();
    let store = lifeops_core::entity::EntityStore::open_in_memory()
        .await
        .unwrap();
    (AppState::new(schemas, pages, store, sdir, vdir), dir)
}
