use crate::schema::{FieldKind, ResolvedField, ResolvedSchema};
use serde_json::{Map, Value};

#[derive(Debug, Clone)]
pub struct FieldError {
    pub field: String,
    pub message: String,
}

#[derive(Debug)]
pub struct ValidationError(pub Vec<FieldError>);

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let list: Vec<String> =
            self.0.iter().map(|e| format!("{}: {}", e.field, e.message)).collect();
        write!(f, "검증 실패: {}", list.join(" / "))
    }
}

impl std::error::Error for ValidationError {}

pub fn validate_entity(
    schema: &ResolvedSchema,
    data: &Map<String, Value>,
) -> Result<(), ValidationError> {
    let mut errors = Vec::new();

    for key in data.keys() {
        if !schema.fields.contains_key(key) {
            errors.push(FieldError {
                field: key.clone(),
                message: format!("타입 '{}'에 없는 필드", schema.name),
            });
        }
    }

    for (fname, fdef) in &schema.fields {
        let value = data.get(fname);
        let missing = matches!(value, None | Some(Value::Null));
        if fdef.required && missing {
            errors.push(FieldError { field: fname.clone(), message: "필수 필드".into() });
            continue;
        }
        if missing {
            continue;
        }
        if let Err(message) = check_kind(&fdef.kind, fdef, value.unwrap()) {
            errors.push(FieldError { field: fname.clone(), message });
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(ValidationError(errors))
    }
}

fn check_kind(kind: &FieldKind, fdef: &ResolvedField, v: &Value) -> Result<(), String> {
    match kind {
        FieldKind::Text | FieldKind::RichText | FieldKind::Image | FieldKind::Ref => {
            v.as_str().map(|_| ()).ok_or_else(|| "문자열이어야 함".into())
        }
        FieldKind::Number => v.as_f64().map(|_| ()).ok_or_else(|| "숫자여야 함".into()),
        FieldKind::Bool => v.as_bool().map(|_| ()).ok_or_else(|| "true/false여야 함".into()),
        FieldKind::Date => {
            let s = v.as_str().ok_or("문자열이어야 함")?;
            chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .map(|_| ())
                .map_err(|_| "YYYY-MM-DD 형식이어야 함".into())
        }
        FieldKind::Url => {
            let s = v.as_str().ok_or("문자열이어야 함")?;
            if s.starts_with("http://") || s.starts_with("https://") {
                Ok(())
            } else {
                Err("http(s):// URL이어야 함".into())
            }
        }
        FieldKind::Enum => {
            let s = v.as_str().ok_or("문자열이어야 함")?;
            let opts = fdef.options.as_deref().unwrap_or(&[]);
            if opts.iter().any(|o| o == s) {
                Ok(())
            } else {
                Err(format!("허용되지 않는 값 '{s}' (허용: {})", opts.join(", ")))
            }
        }
        FieldKind::Money => {
            let o = v.as_object().ok_or("{amount, currency} 객체여야 함")?;
            let amount_ok = o.get("amount").and_then(Value::as_f64).is_some();
            let currency_ok =
                o.get("currency").and_then(Value::as_str).is_some_and(|c| !c.is_empty());
            if amount_ok && currency_ok {
                Ok(())
            } else {
                Err("{amount: 숫자, currency: 문자열} 형식이어야 함".into())
            }
        }
        FieldKind::List(inner) => {
            let arr = v.as_array().ok_or("배열이어야 함")?;
            for (i, item) in arr.iter().enumerate() {
                check_kind(inner, fdef, item).map_err(|m| format!("[{i}]: {m}"))?;
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::SchemaSet;
    use serde_json::{json, Map, Value};

    fn schema() -> crate::schema::ResolvedSchema {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("시계.yaml"),
            "type: 시계\nfields:\n  이름: { kind: text, required: true }\n  가격: { kind: money }\n  구매일: { kind: date }\n  상태: { kind: enum, options: [위시, 보유] }\n  링크: { kind: url }\n  개수: { kind: number }\n  관련: { kind: \"list<ref>\", target: 시계 }\n",
        )
        .unwrap();
        SchemaSet::load_dir(dir.path()).unwrap().get("시계").unwrap().clone()
    }

    fn obj(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn 유효한_데이터는_통과() {
        let data = obj(json!({
            "이름": "세이코 미쿠",
            "가격": { "amount": 650000.0, "currency": "KRW" },
            "구매일": "2026-09-15",
            "상태": "위시",
            "링크": "https://iei.jp/51570/",
            "개수": 1,
            "관련": ["some-id"]
        }));
        assert!(validate_entity(&schema(), &data).is_ok());
    }

    #[test]
    fn 필수_필드_누락() {
        let err = validate_entity(&schema(), &obj(json!({ "상태": "위시" }))).unwrap_err();
        assert!(err.0.iter().any(|e| e.field == "이름" && e.message.contains("필수")));
    }

    #[test]
    fn 스키마에_없는_필드_거부() {
        let err =
            validate_entity(&schema(), &obj(json!({ "이름": "a", "유령필드": 1 }))).unwrap_err();
        assert!(err.0.iter().any(|e| e.field == "유령필드"));
    }

    #[test]
    fn kind별_타입_불일치() {
        let err = validate_entity(
            &schema(),
            &obj(json!({
                "이름": 123,                     // text인데 숫자
                "가격": 650000,                  // money인데 숫자
                "구매일": "2026/09/15",          // 날짜 형식 오류
                "상태": "박살남",                 // enum 밖의 값
                "링크": "ftp://x",               // http(s) 아님
                "관련": "not-an-array"           // list인데 문자열
            })),
        )
        .unwrap_err();
        let fields: Vec<&str> = err.0.iter().map(|e| e.field.as_str()).collect();
        for f in ["이름", "가격", "구매일", "상태", "링크", "관련"] {
            assert!(fields.contains(&f), "{f} 에러가 없음: {fields:?}");
        }
    }

    #[test]
    fn null은_비필수_필드에서_허용() {
        let data = obj(json!({ "이름": "a", "가격": null }));
        assert!(validate_entity(&schema(), &data).is_ok());
    }
}
