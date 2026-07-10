use crate::entity::store::row_to_entity;
use crate::entity::Entity;
use crate::entity::EntityStore;
use crate::error::CoreError;
use crate::schema::{FieldKind, ResolvedSchema};
use crate::schema::SchemaSet;
use serde::Serialize;
use serde_json::Value;

/// richtext(HTML) → 검색용 평문. 태그 제거 + 흔한 엔티티 디코드 + 공백 축약.
pub fn strip_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    // &amp;는 마지막에 디코드(이중 디코드 방지)
    let decoded = out
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 엔티티의 검색 대상 텍스트를 (필드명, 텍스트)로 추출한다.
pub fn searchable_fields(schema: &ResolvedSchema, entity: &Entity) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (fname, fdef) in &schema.fields {
        let Some(value) = entity.data.get(fname) else {
            continue;
        };
        for text in field_texts(&fdef.kind, value) {
            if !text.is_empty() {
                out.push((fname.clone(), text));
            }
        }
    }
    out.push(("타입".to_string(), entity.entity_type.clone()));
    out
}

fn field_texts(kind: &FieldKind, value: &Value) -> Vec<String> {
    match kind {
        FieldKind::Text | FieldKind::Enum => {
            value.as_str().map(|s| vec![s.to_string()]).unwrap_or_default()
        }
        FieldKind::RichText => value.as_str().map(|s| vec![strip_html(s)]).unwrap_or_default(),
        FieldKind::List(inner) if matches!(**inner, FieldKind::Text | FieldKind::Enum) => value
            .as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default(),
        FieldKind::List(inner) if **inner == FieldKind::RichText => value
            .as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(strip_html)).collect())
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// 대소문자 무시 부분 문자열 위치(char 인덱스).
fn find_ci(hay: &[char], needle: &[char]) -> Option<usize> {
    if needle.is_empty() || needle.len() > hay.len() {
        return None;
    }
    (0..=hay.len() - needle.len()).find(|&i| {
        hay[i..i + needle.len()]
            .iter()
            .zip(needle.iter())
            .all(|(a, b)| a.to_lowercase().eq(b.to_lowercase()))
    })
}

/// 매치 주변을 잘라 (스니펫, 매치시작 char오프셋, 매치길이 char수)를 만든다.
pub fn build_snippet(text: &str, token: &str) -> (String, usize, usize) {
    const WINDOW: usize = 30;
    let chars: Vec<char> = text.chars().collect();
    let needle: Vec<char> = token.chars().collect();
    let Some(pos) = find_ci(&chars, &needle) else {
        let end = chars.len().min(WINDOW * 2);
        let mut s: String = chars[..end].iter().collect();
        if end < chars.len() {
            s.push('…');
        }
        return (s, 0, 0);
    };
    let win_start = pos.saturating_sub(WINDOW);
    let win_end = (pos + needle.len() + WINDOW).min(chars.len());
    let mut snippet = String::new();
    let mut start = pos - win_start;
    if win_start > 0 {
        snippet.push('…');
        start += 1;
    }
    snippet.extend(chars[win_start..win_end].iter());
    if win_end < chars.len() {
        snippet.push('…');
    }
    (snippet, start, needle.len())
}

#[derive(Debug, Clone, Serialize)]
pub struct MatchSpan {
    pub start: usize,
    pub len: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    pub id: String,
    #[serde(rename = "type")]
    pub entity_type: String,
    pub category: Option<String>,
    pub label: String,
    pub field: String,
    pub snippet: String,
    #[serde(rename = "match")]
    pub match_span: MatchSpan,
    pub singleton: bool,
    pub href: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchResults {
    pub query: String,
    pub results: Vec<SearchHit>,
    pub total: usize,
    pub truncated: bool,
}

fn like_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

impl EntityStore {
    pub async fn search(
        &self,
        schemas: &SchemaSet,
        query: &str,
        limit: usize,
    ) -> Result<SearchResults, CoreError> {
        let tokens: Vec<String> = query.split_whitespace().map(|t| t.to_lowercase()).collect();
        if tokens.is_empty() {
            return Ok(SearchResults { query: query.to_string(), results: Vec::new(), total: 0, truncated: false });
        }

        // 타입명은 별도 컬럼(data JSON 밖)이므로 프리필터에서 type도 훑어야 타입명 검색이 산다.
        // 토큰별로 data 또는 type에 있으면 후보(토큰 AND 유지). 오탐은 아래 all_present가 제거.
        let clause =
            vec!["(data LIKE ? ESCAPE '\\' OR type LIKE ? ESCAPE '\\')"; tokens.len()].join(" AND ");
        let sql = format!("SELECT id, type, data, created_at, updated_at FROM entities WHERE {clause}");
        let mut q = sqlx::query(&sql);
        for t in &tokens {
            let pat = format!("%{}%", like_escape(t));
            q = q.bind(pat.clone()).bind(pat);
        }
        let rows = q.fetch_all(self.pool()).await?;

        let mut scored: Vec<(u8, String, SearchHit)> = Vec::new();
        for entity in rows.into_iter().map(row_to_entity) {
            let Some(schema) = schemas.get(&entity.entity_type) else {
                continue;
            };
            let fields = searchable_fields(schema, &entity);

            // 모든 토큰이 어느 필드든 존재해야 통과(raw JSON 키·UUID·태그 오탐 제거)
            let all_present = tokens
                .iter()
                .all(|tok| fields.iter().any(|(_, text)| text.to_lowercase().contains(tok.as_str())));
            if !all_present {
                continue;
            }

            let label_field = schema.fields.iter().find(|(_, f)| f.kind == FieldKind::Text).map(|(n, _)| n.clone());
            let weight_of = |fname: &str| -> u8 {
                if fname == "타입" {
                    return 3;
                }
                if label_field.as_deref() == Some(fname) {
                    return 0;
                }
                match schema.fields.get(fname) {
                    Some(f)
                        if f.kind == FieldKind::RichText
                            || matches!(&f.kind, FieldKind::List(inner) if **inner == FieldKind::RichText) =>
                    {
                        2
                    }
                    Some(_) => 1,
                    None => 3,
                }
            };

            // 첫 토큰이 매치된 최소 weight 필드로 스니펫 생성
            let first = &tokens[0];
            let best = fields
                .iter()
                .filter(|(_, text)| text.to_lowercase().contains(first.as_str()))
                .min_by_key(|(fname, _)| weight_of(fname));
            let (field, snippet, start, len, weight) = match best {
                Some((fname, text)) => {
                    let (snip, st, ln) = build_snippet(text, first);
                    (fname.clone(), snip, st, ln, weight_of(fname))
                }
                None => ("타입".to_string(), entity.entity_type.clone(), 0usize, 0usize, 3u8),
            };

            let label = label_field
                .as_ref()
                .and_then(|lf| entity.data.get(lf))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .unwrap_or_else(|| entity.id.chars().take(8).collect());

            let href = if schema.singleton {
                format!("/pages/{}", entity.entity_type)
            } else {
                format!("/entity/{}", entity.id)
            };

            scored.push((
                weight,
                entity.updated_at.clone(),
                SearchHit {
                    id: entity.id,
                    entity_type: entity.entity_type,
                    category: schema.category.clone(),
                    label,
                    field,
                    snippet,
                    match_span: MatchSpan { start, len },
                    singleton: schema.singleton,
                    href,
                },
            ));
        }

        // weight 오름차순, 동급은 updated_at 내림차순(최근 먼저)
        scored.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));

        let total = scored.len();
        let truncated = total > limit;
        let results: Vec<SearchHit> = scored.into_iter().take(limit).map(|(_, _, h)| h).collect();

        Ok(SearchResults { query: query.to_string(), results, total, truncated })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_html_태그_엔티티_공백을_정리한다() {
        assert_eq!(strip_html("<p>안녕 <b>미쿠</b></p>"), "안녕 미쿠");
        assert_eq!(strip_html("a &amp; b &lt;tag&gt;"), "a & b <tag>");
        assert_eq!(strip_html("  여러   공백\n줄바꿈  "), "여러 공백 줄바꿈");
    }

    use crate::schema::SchemaSet;
    use serde_json::json;

    fn note_schema_entity() -> (ResolvedSchema, Entity) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("노트.yaml"),
            "type: 노트\nfields:\n  제목: { kind: text }\n  본문: { kind: richtext }\n  태그: { kind: enum, options: [일기, 회고] }\n  점수: { kind: number }\n",
        )
        .unwrap();
        let set = SchemaSet::load_dir(dir.path()).unwrap();
        let schema = set.get("노트").unwrap().clone();
        let entity = Entity {
            id: "n1".into(),
            entity_type: "노트".into(),
            data: json!({ "제목": "여름 회고", "본문": "<p>세이코를 <b>팔</b>았다</p>", "태그": "회고", "점수": 3 })
                .as_object()
                .unwrap()
                .clone(),
            created_at: String::new(),
            updated_at: String::new(),
        };
        (schema, entity)
    }

    #[test]
    fn searchable_fields_텍스트_richtext_enum_타입명포함_숫자와필드명제외() {
        let (schema, entity) = note_schema_entity();
        let fields = searchable_fields(&schema, &entity);
        let texts: Vec<&str> = fields.iter().map(|(_, t)| t.as_str()).collect();
        assert!(texts.contains(&"여름 회고"));                     // text
        assert!(texts.iter().any(|t| t.contains("세이코를 팔았다"))); // richtext 태그 제거
        assert!(texts.contains(&"회고"));                          // enum 값
        assert!(texts.contains(&"노트"));                          // 타입명
        assert!(!texts.iter().any(|t| t.contains('3')));           // number 제외
        assert!(!texts.contains(&"제목"));                         // 필드명 자체 제외
    }

    #[test]
    fn searchable_fields는_meta_예약키를_검색텍스트에서_제외한다() {
        let (schema, mut entity) = note_schema_entity();
        entity.data.insert(
            "$meta".into(),
            json!({ "제목": { "source": "imported" } }),
        );
        let fields = searchable_fields(&schema, &entity);

        // $meta는 필드로도, 그 안의 "imported" 문자열로도 나오지 않는다.
        assert!(!fields.iter().any(|(f, _)| f == "$meta"));
        assert!(!fields.iter().any(|(_, t)| t.contains("imported")));
    }

    #[test]
    fn searchable_fields_리스트_enum과_richtext는_원소별_추출_url과_ref는_제외() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("장소.yaml"),
            "type: 장소\nfields:\n  태그들: { kind: \"list<enum>\", options: [일기, 회고] }\n  메모들: { kind: \"list<richtext>\" }\n  링크: { kind: url }\n  관련: { kind: \"list<ref>\" }\n",
        )
        .unwrap();
        let set = SchemaSet::load_dir(dir.path()).unwrap();
        let schema = set.get("장소").unwrap().clone();
        let entity = Entity {
            id: "p1".into(),
            entity_type: "장소".into(),
            data: json!({
                "태그들": ["일기", "회고"],
                "메모들": ["<p>첫 <b>메모</b></p>", "<i>둘째</i> 메모"],
                "링크": "세이코공식몰검색어",
                "관련": ["물건77", "물건88"]
            })
            .as_object()
            .unwrap()
            .clone(),
            created_at: String::new(),
            updated_at: String::new(),
        };
        let fields = searchable_fields(&schema, &entity);

        // list<enum>: 각 원소가 자신의 (필드명, 값) 항목으로 나온다
        let tag_texts: Vec<&str> = fields
            .iter()
            .filter(|(f, _)| f.as_str() == "태그들")
            .map(|(_, t)| t.as_str())
            .collect();
        assert_eq!(tag_texts, vec!["일기", "회고"]);

        // list<richtext>: 원소별로 HTML 태그가 제거된다
        let memo_texts: Vec<&str> = fields
            .iter()
            .filter(|(f, _)| f.as_str() == "메모들")
            .map(|(_, t)| t.as_str())
            .collect();
        assert_eq!(memo_texts, vec!["첫 메모", "둘째 메모"]);

        // 제외 kind(url, list<ref>)의 실제 값은 검색 텍스트에 없어야 한다
        let all_texts: Vec<&str> = fields.iter().map(|(_, t)| t.as_str()).collect();
        assert!(!all_texts.iter().any(|t| t.contains("세이코공식몰검색어"))); // url 값 제외
        assert!(!all_texts.iter().any(|t| t.contains("물건77")));            // list<ref> 값 제외
        assert!(!all_texts.iter().any(|t| t.contains("물건88")));

        // 타입명은 여전히 포함된다
        assert!(all_texts.contains(&"장소"));
    }

    #[test]
    fn build_snippet_매치_주변과_오프셋() {
        let (s, start, len) = build_snippet("작년에 산 세이코를 다시 정리했다", "세이코");
        assert_eq!(len, 3);
        assert_eq!(s.chars().skip(start).take(len).collect::<String>(), "세이코");
    }

    #[test]
    fn build_snippet_긴_텍스트는_말줄임() {
        let text = format!("{}세이코{}", "가".repeat(60), "나".repeat(60));
        let (s, start, len) = build_snippet(&text, "세이코");
        assert!(s.starts_with('…'));
        assert!(s.ends_with('…'));
        assert_eq!(s.chars().skip(start).take(len).collect::<String>(), "세이코");
    }

    #[test]
    fn build_snippet_대소문자_무시_매치() {
        // 소문자 토큰이 혼합 대소문자 원문과 매치되고, 오프셋은 원문(원래 대소문자)을 가리킨다.
        let (s, start, len) = build_snippet("My Seiko Watch", "seiko");
        assert_eq!(len, 5);
        assert_eq!(s.chars().skip(start).take(len).collect::<String>(), "Seiko");
    }

    #[test]
    fn build_snippet_무매치는_앞부분과_0오프셋() {
        // 매치가 없으면 앞 60자 이내를 그대로 반환하고 (start, len)=(0, 0).
        let (s, start, len) = build_snippet("짧은 텍스트", "없는토큰");
        assert_eq!(len, 0);
        assert_eq!(start, 0);
        assert_eq!(s, "짧은 텍스트");
    }

    async fn seeded() -> (EntityStore, SchemaSet) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("시계.yaml"), "type: 시계\ncategory: 컬렉션\nfields:\n  이름: { kind: text }\n").unwrap();
        std::fs::write(dir.path().join("노트.yaml"), "type: 노트\ncategory: 메모\nfields:\n  제목: { kind: text }\n  본문: { kind: richtext }\n").unwrap();
        std::fs::write(dir.path().join("프로필.yaml"), "type: 프로필\ncategory: 나\nsingleton: true\nfields:\n  이름: { kind: text }\n  AI메모: { kind: richtext }\n").unwrap();
        std::fs::write(dir.path().join("측정.yaml"), "type: 측정\ncategory: 기록\nfields:\n  항목: { kind: text }\n  값: { kind: number }\n").unwrap();
        std::fs::write(dir.path().join("로그.yaml"), "type: 로그\ncategory: 기록\nfields:\n  기록: { kind: richtext }\n").unwrap();
        let schemas = SchemaSet::load_dir(dir.path()).unwrap();
        let store = EntityStore::open_in_memory().await.unwrap();
        (store, schemas)
    }

    async fn mk(store: &EntityStore, schemas: &SchemaSet, ty: &str, data: serde_json::Value) -> Entity {
        store.create(schemas, ty, data.as_object().unwrap().clone()).await.unwrap()
    }

    #[tokio::test]
    async fn 여러_형태를_가로질러_검색한다() {
        let (store, schemas) = seeded().await;
        mk(&store, &schemas, "시계", json!({ "이름": "세이코 미쿠" })).await;
        mk(&store, &schemas, "노트", json!({ "제목": "여름 회고", "본문": "<p>세이코를 팔았다</p>" })).await;
        mk(&store, &schemas, "측정", json!({ "항목": "세이코 관련 지출", "값": 5 })).await;
        mk(&store, &schemas, "프로필", json!({ "이름": "미쿠" })).await; // 세이코 없음

        let res = store.search(&schemas, "세이코", 50).await.unwrap();
        let types: std::collections::HashSet<&str> = res.results.iter().map(|h| h.entity_type.as_str()).collect();
        assert!(types.contains("시계") && types.contains("노트") && types.contains("측정"));
        assert!(!types.contains("프로필"));
    }

    #[tokio::test]
    async fn 타입명으로도_검색된다() {
        let (store, schemas) = seeded().await;
        // data엔 "시계" 부분문자열이 없지만 타입명이 "시계" → 타입명 매치로 잡혀야(스펙 검색범위: 타입명 포함).
        mk(&store, &schemas, "시계", json!({ "이름": "세이코" })).await;
        mk(&store, &schemas, "노트", json!({ "제목": "무관", "본문": "<p>다른 내용</p>" })).await;
        let res = store.search(&schemas, "시계", 50).await.unwrap();
        assert_eq!(res.total, 1);
        assert_eq!(res.results[0].entity_type, "시계");
        assert_eq!(res.results[0].field, "타입");
    }

    #[tokio::test]
    async fn 필드명은_오탐으로_잡히지_않는다() {
        let (store, schemas) = seeded().await;
        // "AI메모"는 프로필의 필드명. 값엔 그 문자열이 없음 → raw JSON 키로 프리필터는 통과하지만 결과엔 없어야.
        mk(&store, &schemas, "프로필", json!({ "이름": "미쿠", "AI메모": "<p>담백한 톤</p>" })).await;
        let res = store.search(&schemas, "AI메모", 50).await.unwrap();
        assert_eq!(res.total, 0);
    }

    #[tokio::test]
    async fn 토큰_and_모두_포함해야_매치() {
        let (store, schemas) = seeded().await;
        mk(&store, &schemas, "시계", json!({ "이름": "세이코 미쿠" })).await;
        mk(&store, &schemas, "시계", json!({ "이름": "세이코 렌" })).await;
        let res = store.search(&schemas, "세이코 미쿠", 50).await.unwrap();
        assert_eq!(res.results.len(), 1);
        assert_eq!(res.results[0].label, "세이코 미쿠");
    }

    #[tokio::test]
    async fn 싱글턴은_페이지_href_그외는_엔티티_href() {
        let (store, schemas) = seeded().await;
        mk(&store, &schemas, "프로필", json!({ "이름": "미쿠타로" })).await;
        let w = mk(&store, &schemas, "시계", json!({ "이름": "미쿠타로 시계" })).await;
        let res = store.search(&schemas, "미쿠타로", 50).await.unwrap();
        let prof = res.results.iter().find(|h| h.entity_type == "프로필").unwrap();
        assert!(prof.singleton);
        assert_eq!(prof.href, "/pages/프로필");
        let watch = res.results.iter().find(|h| h.entity_type == "시계").unwrap();
        assert_eq!(watch.href, format!("/entity/{}", w.id));
    }

    #[tokio::test]
    async fn 라벨은_첫_text_필드_없으면_id폴백() {
        let (store, schemas) = seeded().await;
        let l = mk(&store, &schemas, "로그", json!({ "기록": "<p>세이코 로그</p>" })).await; // text 필드 없음
        let res = store.search(&schemas, "세이코", 50).await.unwrap();
        let hit = res.results.iter().find(|h| h.entity_type == "로그").unwrap();
        assert_eq!(hit.label, l.id.chars().take(8).collect::<String>());
    }

    #[tokio::test]
    async fn 랭킹은_라벨매치가_본문매치보다_앞() {
        let (store, schemas) = seeded().await;
        mk(&store, &schemas, "노트", json!({ "제목": "무관", "본문": "<p>세이코 언급</p>" })).await; // richtext 매치
        mk(&store, &schemas, "시계", json!({ "이름": "세이코" })).await;                         // 라벨 매치
        let res = store.search(&schemas, "세이코", 50).await.unwrap();
        assert_eq!(res.results[0].entity_type, "시계");
    }

    #[tokio::test]
    async fn 빈_쿼리와_limit_truncated() {
        let (store, schemas) = seeded().await;
        let empty = store.search(&schemas, "   ", 50).await.unwrap();
        assert_eq!(empty.total, 0);
        assert!(empty.results.is_empty());

        for n in ["미쿠 A", "미쿠 B", "미쿠 C"] {
            mk(&store, &schemas, "시계", json!({ "이름": n })).await;
        }
        let res = store.search(&schemas, "미쿠", 2).await.unwrap();
        assert_eq!(res.results.len(), 2);
        assert_eq!(res.total, 3);
        assert!(res.truncated);
    }

    #[test]
    fn like_escape_이스케이프문자_먼저_그다음_와일드카드() {
        // 이스케이프 문자(\)를 먼저 두 배로 만든 뒤 %, _ 와일드카드를 이스케이프해야 한다.
        assert_eq!(like_escape("a%b_c\\d"), "a\\%b\\_c\\\\d");
    }

    #[tokio::test]
    async fn 랭킹은_list_richtext를_tier2로_본다() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("문서.yaml"),
            "type: 문서\ncategory: 자료\nfields:\n  제목: { kind: text }\n  비고: { kind: text }\n  메모들: { kind: \"list<richtext>\" }\n",
        )
        .unwrap();
        let schemas = SchemaSet::load_dir(dir.path()).unwrap();
        let store = EntityStore::open_in_memory().await.unwrap();

        // B를 먼저 만든다. list<richtext>를 tier2로 보정하지 않으면 둘 다 tier1이 되고,
        // 같은 초 타임스탬프에서 안정 정렬이 삽입 순서(B 먼저)를 유지 → B가 앞서 랭킹이 뒤집힌다.
        // 보정 후 B의 메모들 매치는 tier2가 되어 A(비고=tier1)가 확실히 앞선다.
        let b = mk(
            &store,
            &schemas,
            "문서",
            json!({ "제목": "무관제목B", "비고": "관계없음", "메모들": ["<p>핵심어 등장</p>"] }),
        )
        .await;
        let a = mk(
            &store,
            &schemas,
            "문서",
            json!({ "제목": "무관제목A", "비고": "핵심어 포함", "메모들": ["<p>다른내용</p>"] }),
        )
        .await;

        let res = store.search(&schemas, "핵심어", 50).await.unwrap();
        let ia = res.results.iter().position(|h| h.id == a.id).unwrap();
        let ib = res.results.iter().position(|h| h.id == b.id).unwrap();
        assert!(
            ia < ib,
            "일반 text(tier1) 매치가 list<richtext>(tier2) 매치보다 앞서야 한다: A={ia}, B={ib}"
        );
    }
}
