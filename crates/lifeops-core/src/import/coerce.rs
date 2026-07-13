use crate::import::hangul_eq;
use crate::schema::{FieldKind, ResolvedField};
use serde_json::{json, Value};

/// 원시 값을 대상 필드 kind에 맞는 JSON 값으로 강제한다.
/// 실패는 에러가 아니라 `None`(미설정)으로 처리한다.
pub fn coerce(value: &Value, field: &ResolvedField) -> Option<Value> {
    match &field.kind {
        FieldKind::Text | FieldKind::RichText | FieldKind::Ref | FieldKind::Image => {
            let string = value.as_str()?;
            (!string.is_empty()).then(|| Value::String(string.to_string()))
        }
        FieldKind::Number => value.as_f64().map(|number| json!(number)),
        FieldKind::Bool => value.as_bool().map(Value::Bool),
        FieldKind::Url => extract_url(value.as_str()?).map(Value::String),
        FieldKind::Money => {
            let amount = parse_price_krw(value.as_str()?)?;
            Some(json!({ "amount": amount, "currency": "KRW" }))
        }
        FieldKind::Date => {
            let string = value.as_str()?;
            chrono::NaiveDate::parse_from_str(string, "%Y-%m-%d").ok()?;
            Some(Value::String(string.to_string()))
        }
        FieldKind::Enum => {
            let options = field.options.as_deref().unwrap_or(&[]);
            match value {
                Value::String(candidate) => enum_option(candidate, options).map(Value::String),
                Value::Array(items) => items
                    .iter()
                    .filter_map(Value::as_str)
                    .find_map(|candidate| enum_option(candidate, options))
                    .map(Value::String),
                _ => None,
            }
        }
        FieldKind::List(inner) => {
            let element_field = ResolvedField {
                kind: (**inner).clone(),
                required: false,
                options: field.options.clone(),
                target: field.target.clone(),
                unit: field.unit.clone(),
            };
            let items = match value {
                Value::Array(items) => items.clone(),
                other => vec![other.clone()],
            };
            let coerced: Vec<Value> = items
                .iter()
                .filter_map(|item| coerce(item, &element_field))
                .collect();
            (!coerced.is_empty()).then_some(Value::Array(coerced))
        }
    }
}

fn enum_option(candidate: &str, options: &[String]) -> Option<String> {
    options
        .iter()
        .find(|option| hangul_eq(option, candidate))
        .cloned()
}

/// 문자열에서 첫 `http(s)://` URL을 추출한다.
/// 공백이나 Markdown 링크의 종결 문자에서 URL을 끊는다.
pub fn extract_url(string: &str) -> Option<String> {
    let http = string.find("http://");
    let https = string.find("https://");
    let start = match (http, https) {
        (Some(http), Some(https)) => http.min(https),
        (Some(start), None) | (None, Some(start)) => start,
        (None, None) => return None,
    };

    let tail = &string[start..];
    let end = tail
        .find(|character: char| {
            character.is_whitespace() || matches!(character, ')' | '>' | ']' | '"' | '<')
        })
        .unwrap_or(tail.len());
    let url = &tail[..end];

    url.strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .filter(|remainder| !remainder.is_empty())
        .map(|_| url.to_string())
}

/// 문자열의 각 `원` 바로 앞에서 첫 유효 숫자 토큰을 KRW 금액으로 파싱한다.
pub fn parse_price_krw(string: &str) -> Option<f64> {
    for (won, _) in string.match_indices('원') {
        let before = &string[..won];
        let end = before.len();
        let mut start = end;

        for (index, character) in before.char_indices().rev() {
            if character.is_ascii_digit() || character == ',' {
                start = index;
            } else {
                break;
            }
        }

        let token = &before[start..end];
        if !valid_krw_number(token) {
            continue;
        }
        if before[..start]
            .chars()
            .next_back()
            .is_some_and(|boundary| matches!(boundary, '-' | '+' | '.'))
        {
            continue;
        }

        let number: String = token
            .chars()
            .filter(|character| *character != ',')
            .collect();
        let Ok(amount) = number.parse::<f64>() else {
            continue;
        };
        if amount.is_finite() {
            return Some(amount);
        }
    }

    None
}

fn valid_krw_number(token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    if !token.contains(',') {
        return token.chars().all(|character| character.is_ascii_digit());
    }

    let mut groups = token.split(',');
    let first = groups.next().expect("비어 있지 않은 토큰");
    (1..=3).contains(&first.len())
        && first.chars().all(|character| character.is_ascii_digit())
        && groups.all(|group| {
            group.len() == 3 && group.chars().all(|character| character.is_ascii_digit())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::SchemaSet;
    use serde_json::json;

    fn 물건_필드(name: &str) -> crate::schema::ResolvedField {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("물건.yaml"),
            "type: 물건\nfields:\n  이름: { kind: text, required: true }\n  가격: { kind: money }\n  구매링크: { kind: url }\n  상태: { kind: enum, options: [위시, 보유] }\n  카테고리: { kind: enum, options: [시계, 전자제품, 수집품] }\n  태그: { kind: \"list<text>\" }\n",
        )
        .unwrap();
        SchemaSet::load_dir(dir.path())
            .unwrap()
            .get("물건")
            .unwrap()
            .fields[name]
            .clone()
    }

    #[test]
    fn url_추출() {
        assert_eq!(
            extract_url("[Mercari](<https://jp.mercari.com/item/m1>)").as_deref(),
            Some("https://jp.mercari.com/item/m1")
        );
        assert_eq!(
            extract_url("보기 https://x.com/a/status/1 끝").as_deref(),
            Some("https://x.com/a/status/1")
        );
        assert_eq!(extract_url("http://x").as_deref(), Some("http://x"));
        assert_eq!(
            extract_url("https://first.example http://second.example").as_deref(),
            Some("https://first.example")
        );
        assert_eq!(
            extract_url("[보기](https://x/closed]").as_deref(),
            Some("https://x/closed")
        );
        assert_eq!(extract_url("https://").as_deref(), None);
        assert_eq!(extract_url("링크 없음").as_deref(), None);
    }

    #[test]
    fn 가격_파서() {
        assert_eq!(parse_price_krw("425,414원"), Some(425414.0));
        assert_eq!(parse_price_krw("60 USD(= 86,800원)"), Some(86800.0));
        assert_eq!(parse_price_krw("원가 10,000원"), Some(10000.0));
        assert_eq!(parse_price_krw("지원금 10,000원"), Some(10000.0));
        assert_eq!(parse_price_krw("12.5원, 정상 가격 10,000원"), Some(10000.0));
        for malformed in ["12.5원", "-100원", "+100원", "1,,2원"] {
            assert_eq!(parse_price_krw(malformed), None, "잘못된 가격: {malformed}");
        }
        let overflow = format!("{}원", "9".repeat(400));
        assert_eq!(parse_price_krw(&overflow), None);
        assert_eq!(parse_price_krw("49,800 JPY"), None);
        assert_eq!(parse_price_krw(""), None);
    }

    #[test]
    fn money로_강제하면_amount_currency_객체() {
        let v = coerce(&json!("425,414원"), &물건_필드("가격")).unwrap();
        assert_eq!(v, json!({ "amount": 425414.0, "currency": "KRW" }));
        assert_eq!(coerce(&json!("49,800 JPY"), &물건_필드("가격")), None);
    }

    #[test]
    fn url로_강제() {
        let v = coerce(&json!("[m](<https://x/1>)"), &물건_필드("구매링크")).unwrap();
        assert_eq!(v, json!("https://x/1"));
        assert_eq!(coerce(&json!(""), &물건_필드("구매링크")), None);
    }

    #[test]
    fn enum_멤버십과_리스트_첫유효값() {
        assert_eq!(
            coerce(&json!("위시"), &물건_필드("상태")),
            Some(json!("위시"))
        );
        assert_eq!(coerce(&json!("박살"), &물건_필드("상태")), None);
        let v = coerce(
            &json!(["모바일", "전자제품", "럭셔리"]),
            &물건_필드("카테고리"),
        );
        assert_eq!(v, Some(json!("전자제품")));
        assert_eq!(
            coerce(&json!(["없음1", "없음2"]), &물건_필드("카테고리")),
            None
        );
    }

    #[test]
    fn nfd_enum은_nfc_스키마_옵션으로_강제() {
        let v = coerce(&json!(["전자제품"]), &물건_필드("카테고리"));
        assert_eq!(v, Some(json!("전자제품")));
    }

    #[test]
    fn list_text_강제() {
        assert_eq!(
            coerce(&json!(["a", "b"]), &물건_필드("태그")),
            Some(json!(["a", "b"]))
        );
        assert_eq!(
            coerce(&json!("solo"), &물건_필드("태그")),
            Some(json!(["solo"]))
        );
        assert_eq!(
            coerce(&json!(["", 1, "b"]), &물건_필드("태그")),
            Some(json!(["b"]))
        );
        assert_eq!(coerce(&json!(["", 1]), &물건_필드("태그")), None);
    }
}
