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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_html_태그_엔티티_공백을_정리한다() {
        assert_eq!(strip_html("<p>안녕 <b>미쿠</b></p>"), "안녕 미쿠");
        assert_eq!(strip_html("a &amp; b &lt;tag&gt;"), "a & b <tag>");
        assert_eq!(strip_html("  여러   공백\n줄바꿈  "), "여러 공백 줄바꿈");
    }
}
