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
        let opts = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true);
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
        check_ref_targets(&mut tx, schemas, &edges).await?;
        sqlx::query(
            "INSERT INTO entities (id, type, data, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
        )
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
        let row =
            sqlx::query("SELECT id, type, data, created_at, updated_at FROM entities WHERE id = ?")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(row_to_entity))
    }

    pub async fn update(
        &self,
        schemas: &SchemaSet,
        id: &str,
        patch: Map<String, Value>,
    ) -> Result<Entity, CoreError> {
        let mut entity = self
            .get(id)
            .await?
            .ok_or_else(|| CoreError::NotFound(id.to_string()))?;
        let schema = schemas
            .get(&entity.entity_type)
            .ok_or_else(|| CoreError::UnknownType(entity.entity_type.clone()))?;

        for (k, v) in patch {
            if v.is_null() {
                entity.data.remove(&k);
            } else {
                entity.data.insert(k, v);
            }
        }
        validate_entity(schema, &entity.data)?;
        let edges = collect_refs(schema, &entity.data);
        entity.updated_at = now_rfc3339();

        let mut tx = self.pool.begin().await?;
        check_ref_targets(&mut tx, schemas, &edges).await?;
        sqlx::query("UPDATE entities SET data = ?, updated_at = ? WHERE id = ?")
            .bind(serde_json::Value::Object(entity.data.clone()).to_string())
            .bind(&entity.updated_at)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM refs WHERE from_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        insert_refs(&mut tx, id, &edges).await?;
        tx.commit().await?;
        Ok(entity)
    }

    pub async fn backlinks(&self, id: &str) -> Result<Vec<RefEdge>, CoreError> {
        let rows = sqlx::query(
            "SELECT r.from_id, e.type AS from_type, r.field_name
             FROM refs r JOIN entities e ON e.id = r.from_id
             WHERE r.to_id = ?
             ORDER BY e.updated_at DESC",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| RefEdge {
                from_id: row.get("from_id"),
                from_type: row.get("from_type"),
                field_name: row.get("field_name"),
            })
            .collect())
    }

    pub async fn delete(&self, id: &str) -> Result<(), CoreError> {
        let mut tx = self.pool.begin().await?;
        let exists = sqlx::query("SELECT 1 FROM entities WHERE id = ?")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?
            .is_some();
        if !exists {
            return Err(CoreError::NotFound(id.to_string()));
        }
        let rows = sqlx::query(
            "SELECT r.from_id, e.type AS from_type, r.field_name
             FROM refs r JOIN entities e ON e.id = r.from_id
             WHERE r.to_id = ?
             ORDER BY e.updated_at DESC",
        )
        .bind(id)
        .fetch_all(&mut *tx)
        .await?;
        let referrers: Vec<_> = rows
            .into_iter()
            .map(|row| RefEdge {
                from_id: row.get("from_id"),
                from_type: row.get("from_type"),
                field_name: row.get("field_name"),
            })
            .collect();
        if !referrers.is_empty() {
            return Err(CoreError::DeleteBlocked { referrers });
        }
        sqlx::query("DELETE FROM refs WHERE from_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM entities WHERE id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn list(&self, types: &[String]) -> Result<Vec<Entity>, CoreError> {
        if types.is_empty() {
            return Ok(Vec::new());
        }
        // 필드명·타입명은 쿼리에 직접 넣지 않는다 — placeholder만 조립, 값은 바인딩
        let placeholders = vec!["?"; types.len()].join(", ");
        let sql = format!(
            "SELECT id, type, data, created_at, updated_at FROM entities \
             WHERE type IN ({placeholders}) ORDER BY updated_at DESC"
        );
        let mut query = sqlx::query(&sql);
        for t in types {
            query = query.bind(t);
        }
        let rows = query.fetch_all(&self.pool).await?;
        Ok(rows.into_iter().map(row_to_entity).collect())
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

/// ref/list<ref> 필드에서 (필드명, 대상 id, 선언 target 타입) 목록 추출
pub(crate) fn collect_refs(
    schema: &ResolvedSchema,
    data: &Map<String, Value>,
) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for (fname, fdef) in &schema.fields {
        if !fdef.kind.contains_ref() {
            continue;
        }
        let Some(target) = &fdef.target else {
            continue;
        };
        match data.get(fname) {
            Some(Value::String(id)) if fdef.kind == FieldKind::Ref => {
                out.push((fname.clone(), id.clone(), target.clone()));
            }
            Some(Value::Array(items)) => {
                for item in items {
                    if let Value::String(id) = item {
                        out.push((fname.clone(), id.clone(), target.clone()));
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
    schemas: &SchemaSet,
    edges: &[(String, String, String)],
) -> Result<(), CoreError> {
    for (field, to_id, target) in edges {
        let row = sqlx::query("SELECT type FROM entities WHERE id = ?")
            .bind(to_id)
            .fetch_optional(&mut **tx)
            .await?;
        let Some(row) = row else {
            return Err(CoreError::Validation(ValidationError(vec![FieldError {
                field: field.clone(),
                message: format!("존재하지 않는 엔티티를 참조함: {to_id}"),
            }])));
        };
        let actual: String = row.get("type");
        if !schemas.family_of(target).iter().any(|t| t == &actual) {
            return Err(CoreError::Validation(ValidationError(vec![FieldError {
                field: field.clone(),
                message: format!(
                    "참조 대상 타입 불일치: '{to_id}'는 '{actual}' 타입인데 '{target}' 타입 또는 하위 타입이어야 함"
                ),
            }])));
        }
    }
    Ok(())
}

async fn insert_refs(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    from_id: &str,
    edges: &[(String, String, String)],
) -> Result<(), CoreError> {
    for (field, to_id, _target) in edges {
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

    fn assert_ref_type_mismatch_message(message: &str, referenced_id: &str) {
        assert!(message.contains("타입"), "메시지: {message}");
        assert!(message.contains(referenced_id), "메시지: {message}");
        assert!(message.contains("할일"), "메시지: {message}");
        assert!(message.contains("물건"), "메시지: {message}");
    }

    #[tokio::test]
    async fn 생성하고_조회한다() {
        let store = EntityStore::open_in_memory().await.unwrap();
        let schemas = test_schemas();
        let e = store
            .create(
                &schemas,
                "시계",
                obj(json!({ "이름": "세이코 미쿠", "상태": "위시" })),
            )
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
        let err = store
            .create(&schemas, "유령", obj(json!({})))
            .await
            .unwrap_err();
        assert!(matches!(err, CoreError::UnknownType(_)));
        let err = store
            .create(&schemas, "시계", obj(json!({ "상태": "위시" })))
            .await
            .unwrap_err();
        assert!(matches!(err, CoreError::Validation(_)));
    }

    #[tokio::test]
    async fn ref는_대상이_존재해야_한다() {
        let store = EntityStore::open_in_memory().await.unwrap();
        let schemas = test_schemas();
        let err = store
            .create(
                &schemas,
                "할일",
                obj(json!({ "내용": "에코작", "관련물건": ["ghost-id"] })),
            )
            .await
            .unwrap_err();
        let CoreError::Validation(v) = err else {
            panic!("Validation이어야 함")
        };
        assert!(v.0[0].message.contains("ghost-id"));

        let watch = store
            .create(&schemas, "시계", obj(json!({ "이름": "세이코 미쿠" })))
            .await
            .unwrap();
        let todo = store
            .create(
                &schemas,
                "할일",
                obj(json!({ "내용": "에코작", "관련물건": [watch.id] })),
            )
            .await
            .unwrap();
        assert!(!todo.id.is_empty());
    }

    #[tokio::test]
    async fn ref_대상의_타입이_선언과_다르면_거부() {
        let store = EntityStore::open_in_memory().await.unwrap();
        let schemas = test_schemas();
        // 할일.관련물건 target=물건 인데, 할일 id(물건 family 아님)를 넣으면 거부되어야 한다.
        let todo1 = store
            .create(&schemas, "할일", obj(json!({ "내용": "먼저" })))
            .await
            .unwrap();
        let wrong_id = todo1.id.clone();
        let err = store
            .create(
                &schemas,
                "할일",
                obj(json!({ "내용": "잘못된참조", "관련물건": [wrong_id.clone()] })),
            )
            .await
            .unwrap_err();
        let CoreError::Validation(v) = err else {
            panic!("Validation이어야 함")
        };
        assert_ref_type_mismatch_message(&v.0[0].message, &wrong_id);

        // 물건 family(시계 포함)는 통과
        let watch = store
            .create(&schemas, "시계", obj(json!({ "이름": "시계1" })))
            .await
            .unwrap();
        let ok = store
            .create(
                &schemas,
                "할일",
                obj(json!({ "내용": "정상", "관련물건": [watch.id] })),
            )
            .await;
        assert!(ok.is_ok());
    }

    #[tokio::test]
    async fn ref_대상의_타입이_선언과_다르면_수정도_거부() {
        let store = EntityStore::open_in_memory().await.unwrap();
        let schemas = test_schemas();
        let wrong_todo = store
            .create(&schemas, "할일", obj(json!({ "내용": "잘못된 대상" })))
            .await
            .unwrap();
        let wrong_id = wrong_todo.id.clone();
        let watch = store
            .create(&schemas, "시계", obj(json!({ "이름": "시계1" })))
            .await
            .unwrap();
        let todo = store
            .create(
                &schemas,
                "할일",
                obj(json!({ "내용": "수정 대상", "관련물건": [watch.id] })),
            )
            .await
            .unwrap();

        let err = store
            .update(
                &schemas,
                &todo.id,
                obj(json!({ "관련물건": [wrong_id.clone()] })),
            )
            .await
            .unwrap_err();
        let CoreError::Validation(v) = err else {
            panic!("Validation이어야 함")
        };
        assert_ref_type_mismatch_message(&v.0[0].message, &wrong_id);
    }

    #[tokio::test]
    async fn 없는_id_조회는_none() {
        let store = EntityStore::open_in_memory().await.unwrap();
        assert!(store.get("ghost").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn 부분_수정과_null_제거() {
        let store = EntityStore::open_in_memory().await.unwrap();
        let schemas = test_schemas();
        let e = store
            .create(
                &schemas,
                "시계",
                obj(json!({ "이름": "세이코 미쿠", "상태": "위시", "무브먼트": "쿼츠" })),
            )
            .await
            .unwrap();
        let updated = store
            .update(
                &schemas,
                &e.id,
                obj(json!({ "상태": "주문됨", "무브먼트": null })),
            )
            .await
            .unwrap();
        assert_eq!(updated.data["상태"], "주문됨");
        assert_eq!(updated.data["이름"], "세이코 미쿠"); // 언급 안 한 필드 유지
        assert!(!updated.data.contains_key("무브먼트")); // null → 제거
        assert!(updated.updated_at >= updated.created_at);
    }

    #[tokio::test]
    async fn 수정_결과도_검증된다() {
        let store = EntityStore::open_in_memory().await.unwrap();
        let schemas = test_schemas();
        let e = store
            .create(&schemas, "시계", obj(json!({ "이름": "세이코 미쿠" })))
            .await
            .unwrap();
        // 필수 필드를 null로 지우려는 시도 → 검증 실패
        let err = store
            .update(&schemas, &e.id, obj(json!({ "이름": null })))
            .await
            .unwrap_err();
        assert!(matches!(err, CoreError::Validation(_)));
        // 없는 id → NotFound
        let err = store
            .update(&schemas, "ghost", obj(json!({})))
            .await
            .unwrap_err();
        assert!(matches!(err, CoreError::NotFound(_)));
    }

    #[tokio::test]
    async fn 수정_시_refs가_재구축된다() {
        let store = EntityStore::open_in_memory().await.unwrap();
        let schemas = test_schemas();
        let w1 = store
            .create(&schemas, "시계", obj(json!({ "이름": "시계1" })))
            .await
            .unwrap();
        let w2 = store
            .create(&schemas, "시계", obj(json!({ "이름": "시계2" })))
            .await
            .unwrap();
        let todo = store
            .create(
                &schemas,
                "할일",
                obj(json!({ "내용": "비교", "관련물건": [w1.id.clone()] })),
            )
            .await
            .unwrap();
        store
            .update(
                &schemas,
                &todo.id,
                obj(json!({ "관련물건": [w2.id.clone()] })),
            )
            .await
            .unwrap();
        let back1 = store.backlinks(&w1.id).await.unwrap();
        let back2 = store.backlinks(&w2.id).await.unwrap();
        assert!(back1.is_empty());
        assert_eq!(back2.len(), 1);
        assert_eq!(back2[0].from_id, todo.id);
    }

    #[tokio::test]
    async fn 참조된_엔티티는_삭제가_차단된다() {
        let store = EntityStore::open_in_memory().await.unwrap();
        let schemas = test_schemas();
        let watch = store
            .create(&schemas, "시계", obj(json!({ "이름": "세이코 미쿠" })))
            .await
            .unwrap();
        let todo = store
            .create(
                &schemas,
                "할일",
                obj(json!({ "내용": "개봉", "관련물건": [watch.id.clone()] })),
            )
            .await
            .unwrap();

        let err = store.delete(&watch.id).await.unwrap_err();
        let CoreError::DeleteBlocked { referrers } = err else {
            panic!("DeleteBlocked여야 함")
        };
        assert_eq!(referrers.len(), 1);
        assert_eq!(referrers[0].from_id, todo.id);
        assert_eq!(referrers[0].from_type, "할일");
        assert_eq!(referrers[0].field_name, "관련물건");

        // 참조하는 쪽을 먼저 지우면 삭제 가능해진다
        store.delete(&todo.id).await.unwrap();
        store.delete(&watch.id).await.unwrap();
        assert!(store.get(&watch.id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn 없는_엔티티_삭제는_notfound() {
        let store = EntityStore::open_in_memory().await.unwrap();
        let err = store.delete("ghost").await.unwrap_err();
        assert!(matches!(err, CoreError::NotFound(_)));
    }

    #[tokio::test]
    async fn 타입_목록으로_조회한다() {
        let store = EntityStore::open_in_memory().await.unwrap();
        let schemas = test_schemas();
        store
            .create(&schemas, "물건", obj(json!({ "이름": "잡화1" })))
            .await
            .unwrap();
        store
            .create(&schemas, "시계", obj(json!({ "이름": "시계1" })))
            .await
            .unwrap();
        store
            .create(&schemas, "할일", obj(json!({ "내용": "정리" })))
            .await
            .unwrap();

        let family = schemas.family_of("물건"); // [물건, 시계]
        let items = store.list(&family).await.unwrap();
        assert_eq!(items.len(), 2);
        let all = store
            .list(&["물건".into(), "시계".into(), "할일".into()])
            .await
            .unwrap();
        assert_eq!(all.len(), 3);
        let none = store.list(&[]).await.unwrap();
        assert!(none.is_empty());
    }
}
