use crate::entity::Entity;
use crate::schema::{FieldKind, ResolvedSchema};
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
}
