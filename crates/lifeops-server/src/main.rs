mod app;
mod backup;
mod error;
mod routes;
mod state;
mod static_files;

use lifeops_core::entity::EntityStore;
use lifeops_core::schema::SchemaSet;
use lifeops_core::view::PageSet;
use std::path::Path;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().init();

    let schemas_dir = Path::new("schemas").to_path_buf();
    let views_dir = Path::new("views").to_path_buf();
    let categories_path = Path::new("categories.yaml").to_path_buf();
    let schemas = SchemaSet::load_dir(&schemas_dir).expect("schemas/ 로드 실패");
    let pages = PageSet::load_dir(&views_dir).expect("views/ 로드 실패");
    let categories =
        lifeops_core::schema::load_categories(&categories_path).expect("categories.yaml 로드 실패");
    let store = EntityStore::open(Path::new("data/lifeops.db"))
        .await
        .expect("DB 열기 실패");
    let state = state::AppState::new(
        schemas,
        pages,
        categories,
        store,
        schemas_dir,
        views_dir,
        categories_path,
    );

    backup::spawn_daily_backup(
        std::path::PathBuf::from("data/lifeops.db"),
        std::path::PathBuf::from("backups"),
        7,
    );

    let app = app::build_app(state);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .expect("바인드 실패");
    tracing::info!("LifeOps 서버 http://0.0.0.0:3000");
    axum::serve(listener, app).await.expect("서버 실행 실패");
}
