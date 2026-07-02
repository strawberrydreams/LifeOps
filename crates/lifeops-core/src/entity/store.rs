use crate::entity::validate::{validate_entity, FieldError, ValidationError};
use crate::error::CoreError;
use crate::schema::{FieldKind, ResolvedSchema, SchemaSet};
use serde_json::{Map, Value};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use std::path::Path;

#[derive(Debug, Clone, serde::Serialize)]
pub struct Entity {
    pub id: String,
    #[serde(rename = "type")]
    pub entity_type: String,
    pub data: Map<String, Value>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RefEdge {
    pub from_id: String,
    pub from_type: String,
    pub field_name: String,
}

const MIGRATION: &str = "
CREATE TABLE IF NOT EXISTS entities (
  id TEXT PRIMARY KEY,
  type TEXT NOT NULL,
  data TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_entities_type ON entities(type);
CREATE TABLE IF NOT EXISTS refs (
  from_id TEXT NOT NULL,
  to_id TEXT NOT NULL,
  field_name TEXT NOT NULL,
  PRIMARY KEY (from_id, to_id, field_name)
);
CREATE INDEX IF NOT EXISTS idx_refs_to ON refs(to_id);
";

pub struct EntityStore {
    pool: SqlitePool,
}

impl EntityStore {
    pub async fn open(path: &Path) -> Result<Self, CoreError> {
        let opts = SqliteConnectOptions::new().filename(path).create_if_missing(true);
        let pool = SqlitePoolOptions::new().connect_with(opts).await?;
        Self::init(pool).await
    }

    /// 테스트용. in-memory SQLite는 커넥션마다 별개 DB이므로 커넥션을 1개로 고정한다.
    pub async fn open_in_memory() -> Result<Self, CoreError> {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await?;
        Self::init(pool).await
    }

    async fn init(pool: SqlitePool) -> Result<Self, CoreError> {
        sqlx::raw_sql(MIGRATION).execute(&pool).await?;
        Ok(EntityStore { pool })
    }

    pub async fn create(
        &self,
        schemas: &SchemaSet,
        entity_type: &str,
        data: Map<String, Value>,
    ) -> Result<Entity, CoreError> {
        let schema = schemas
            .get(entity_type)
            .ok_or_else(|| CoreError::UnknownType(entity_type.to_string()))?;
        validate_entity(schema, &data)?;
        let edges = collect_refs(schema, &data);
        let now = now_rfc3339();
        let entity = Entity {
            id: uuid::Uuid::new_v4().to_string(),
            entity_type: entity_type.to_string(),
            data,
            created_at: now.clone(),
            updated_at: now,
        };

        let mut tx = self.pool.begin().await?;
        check_ref_targets(&mut tx, &edges).await?;
        sqlx::query("INSERT INTO entities (id, type, data, created_at, updated_at) VALUES (?, ?, ?, ?, ?)")
            .bind(&entity.id)
            .bind(&entity.entity_type)
            .bind(serde_json::Value::Object(entity.data.clone()).to_string())
            .bind(&entity.created_at)
            .bind(&entity.updated_at)
            .execute(&mut *tx)
            .await?;
        insert_refs(&mut tx, &entity.id, &edges).await?;
        tx.commit().await?;
        Ok(entity)
    }

    pub async fn get(&self, id: &str) -> Result<Option<Entity>, CoreError> {
        let row = sqlx::query("SELECT id, type, data, created_at, updated_at FROM entities WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(row_to_entity))
    }
}

pub(crate) fn row_to_entity(row: sqlx::sqlite::SqliteRow) -> Entity {
    let data: Map<String, Value> =
        serde_json::from_str(&row.get::<String, _>("data")).unwrap_or_default();
    Entity {
        id: row.get("id"),
        entity_type: row.get("type"),
        data,
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

pub(crate) fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// ref/list<ref> 필드에서 (필드명, 대상 id) 목록 추출
pub(crate) fn collect_refs(schema: &ResolvedSchema, data: &Map<String, Value>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (fname, fdef) in &schema.fields {
        if !fdef.kind.contains_ref() {
            continue;
        }
        match data.get(fname) {
            Some(Value::String(id)) if fdef.kind == FieldKind::Ref => {
                out.push((fname.clone(), id.clone()));
            }
            Some(Value::Array(items)) => {
                for item in items {
                    if let Value::String(id) = item {
                        out.push((fname.clone(), id.clone()));
                    }
                }
            }
            _ => {}
        }
    }
    out
}

async fn check_ref_targets(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    edges: &[(String, String)],
) -> Result<(), CoreError> {
    for (field, to_id) in edges {
        let exists = sqlx::query("SELECT 1 FROM entities WHERE id = ?")
            .bind(to_id)
            .fetch_optional(&mut **tx)
            .await?
            .is_some();
        if !exists {
            return Err(CoreError::Validation(ValidationError(vec![FieldError {
                field: field.clone(),
                message: format!("존재하지 않는 엔티티를 참조함: {to_id}"),
            }])));
        }
    }
    Ok(())
}

async fn insert_refs(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    from_id: &str,
    edges: &[(String, String)],
) -> Result<(), CoreError> {
    for (field, to_id) in edges {
        sqlx::query("INSERT OR IGNORE INTO refs (from_id, to_id, field_name) VALUES (?, ?, ?)")
            .bind(from_id)
            .bind(to_id)
            .bind(field)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::SchemaSet;
    use serde_json::{json, Map, Value};

    pub(crate) fn test_schemas() -> SchemaSet {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("물건.yaml"),
            "type: 물건\nfields:\n  이름: { kind: text, required: true }\n  상태: { kind: enum, options: [위시, 주문됨, 보유, 과거] }\n  가격: { kind: money }\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("시계.yaml"),
            "type: 시계\nextends: 물건\nfields:\n  무브먼트: { kind: text }\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("할일.yaml"),
            "type: 할일\nfields:\n  내용: { kind: text, required: true }\n  완료: { kind: bool }\n  관련물건: { kind: \"list<ref>\", target: 물건 }\n",
        )
        .unwrap();
        SchemaSet::load_dir(dir.path()).unwrap()
    }

    pub(crate) fn obj(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[tokio::test]
    async fn 생성하고_조회한다() {
        let store = EntityStore::open_in_memory().await.unwrap();
        let schemas = test_schemas();
        let e = store
            .create(&schemas, "시계", obj(json!({ "이름": "세이코 미쿠", "상태": "위시" })))
            .await
            .unwrap();
        assert!(!e.id.is_empty());
        let loaded = store.get(&e.id).await.unwrap().unwrap();
        assert_eq!(loaded.entity_type, "시계");
        assert_eq!(loaded.data["이름"], "세이코 미쿠");
        assert_eq!(loaded.created_at, loaded.updated_at);
    }

    #[tokio::test]
    async fn 없는_타입과_검증_실패() {
        let store = EntityStore::open_in_memory().await.unwrap();
        let schemas = test_schemas();
        let err = store.create(&schemas, "유령", obj(json!({}))).await.unwrap_err();
        assert!(matches!(err, CoreError::UnknownType(_)));
        let err = store.create(&schemas, "시계", obj(json!({ "상태": "위시" }))).await.unwrap_err();
        assert!(matches!(err, CoreError::Validation(_)));
    }

    #[tokio::test]
    async fn ref는_대상이_존재해야_한다() {
        let store = EntityStore::open_in_memory().await.unwrap();
        let schemas = test_schemas();
        let err = store
            .create(&schemas, "할일", obj(json!({ "내용": "에코작", "관련물건": ["ghost-id"] })))
            .await
            .unwrap_err();
        let CoreError::Validation(v) = err else { panic!("Validation이어야 함") };
        assert!(v.0[0].message.contains("ghost-id"));

        let watch = store
            .create(&schemas, "시계", obj(json!({ "이름": "세이코 미쿠" })))
            .await
            .unwrap();
        let todo = store
            .create(&schemas, "할일", obj(json!({ "내용": "에코작", "관련물건": [watch.id] })))
            .await
            .unwrap();
        assert!(!todo.id.is_empty());
    }

    #[tokio::test]
    async fn 없는_id_조회는_none() {
        let store = EntityStore::open_in_memory().await.unwrap();
        assert!(store.get("ghost").await.unwrap().is_none());
    }
}
