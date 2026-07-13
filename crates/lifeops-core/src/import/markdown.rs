use serde_json::{Map, Value};

#[derive(Debug, Clone)]
pub struct ParsedDoc {
    pub frontmatter: Map<String, Value>,
    pub body: String,
    pub frontmatter_ok: bool,
}

/// 원문 문자열을 프론트매터(YAML)와 본문으로 가른다.
/// - CRLF는 LF로 정규화한다(OneDrive 대응).
/// - 첫 줄이 `---`이고 이후 `---`로 닫히면 그 사이를 YAML로 파싱한다.
/// - 프론트매터가 없거나 YAML 파싱이 실패하면 전체(또는 닫는 --- 이후)를 본문으로 강등한다.
pub fn parse_document(content: &str) -> ParsedDoc {
    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");

    let no_front = || ParsedDoc {
        frontmatter: Map::new(),
        body: normalized.clone(),
        frontmatter_ok: false,
    };

    let rest = match normalized.strip_prefix("---\n") {
        Some(r) => r,
        None => return no_front(),
    };
    // 닫는 구분선: 줄 시작의 "---"
    let Some(end) = find_closing_fence(rest) else {
        return no_front();
    };
    let (front_src, after) = rest.split_at(end);
    // 닫는 --- 줄과 그 줄의 개행을 건너뛴 나머지가 본문
    let after_fence = &after["---".len()..];
    let body = after_fence.strip_prefix('\n').unwrap_or(after_fence);
    let body = body.strip_prefix('\n').unwrap_or(body);

    match serde_yaml::from_str::<serde_json::Value>(front_src) {
        Ok(Value::Object(map)) => ParsedDoc {
            frontmatter: map,
            body: body.to_string(),
            frontmatter_ok: true,
        },
        _ => ParsedDoc {
            frontmatter: Map::new(),
            body: body.to_string(),
            frontmatter_ok: false,
        },
    }
}

/// `rest`에서 줄 시작의 `---` 구분선 시작 바이트 오프셋을 찾는다.
fn find_closing_fence(rest: &str) -> Option<usize> {
    let mut offset = 0usize;
    for line in rest.split_inclusive('\n') {
        let trimmed = line.strip_suffix('\n').unwrap_or(line);
        if trimmed == "---" {
            return Some(offset);
        }
        offset += line.len();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 프론트매터와_본문을_가른다() {
        let doc = parse_document("---\ntitle: 권한\ntags: [linux]\n---\n\n# 본문\n내용");
        assert!(doc.frontmatter_ok);
        assert_eq!(doc.frontmatter["title"], serde_json::json!("권한"));
        assert_eq!(doc.frontmatter["tags"], serde_json::json!(["linux"]));
        assert_eq!(doc.body, "# 본문\n내용");
    }

    #[test]
    fn crlf는_정규화된다() {
        let doc = parse_document("---\r\ntitle: A\r\n---\r\n본문\r\n");
        assert!(doc.frontmatter_ok);
        assert_eq!(doc.frontmatter["title"], serde_json::json!("A"));
        assert_eq!(doc.body, "본문\n");
    }

    #[test]
    fn 프론트매터가_없으면_전체가_본문() {
        let doc = parse_document("# 제목만 있는 문서\n본문");
        assert!(!doc.frontmatter_ok);
        assert!(doc.frontmatter.is_empty());
        assert_eq!(doc.body, "# 제목만 있는 문서\n본문");
    }

    #[test]
    fn yaml_파싱_실패는_본문으로_강등() {
        // 닫는 --- 는 있으나 프론트매터 YAML이 깨진 경우
        let doc = parse_document("---\ntitle: [닫히지 않은\n---\n본문");
        assert!(!doc.frontmatter_ok);
        assert!(doc.frontmatter.is_empty());
        assert_eq!(doc.body, "본문");
    }
}
