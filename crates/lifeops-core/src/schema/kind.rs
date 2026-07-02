#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldKind {
    Text,
    RichText,
    Number,
    Money,
    Date,
    Bool,
    Enum,
    Url,
    Ref,
    Image,
    List(Box<FieldKind>),
}

impl std::fmt::Display for FieldKind {
    /// YAML 표기와 동일한 문자열 형태 ("text", "list<ref>") — API 직렬화에도 사용
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FieldKind::Text => write!(f, "text"),
            FieldKind::RichText => write!(f, "richtext"),
            FieldKind::Number => write!(f, "number"),
            FieldKind::Money => write!(f, "money"),
            FieldKind::Date => write!(f, "date"),
            FieldKind::Bool => write!(f, "bool"),
            FieldKind::Enum => write!(f, "enum"),
            FieldKind::Url => write!(f, "url"),
            FieldKind::Ref => write!(f, "ref"),
            FieldKind::Image => write!(f, "image"),
            FieldKind::List(inner) => write!(f, "list<{inner}>"),
        }
    }
}

impl serde::Serialize for FieldKind {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl FieldKind {
    pub fn parse(s: &str) -> Option<FieldKind> {
        let s = s.trim();
        if let Some(inner) = s.strip_prefix("list<").and_then(|r| r.strip_suffix('>')) {
            let inner = FieldKind::parse(inner)?;
            if matches!(inner, FieldKind::List(_)) {
                return None; // 중첩 리스트 금지
            }
            return Some(FieldKind::List(Box::new(inner)));
        }
        match s {
            "text" => Some(FieldKind::Text),
            "richtext" => Some(FieldKind::RichText),
            "number" => Some(FieldKind::Number),
            "money" => Some(FieldKind::Money),
            "date" => Some(FieldKind::Date),
            "bool" => Some(FieldKind::Bool),
            "enum" => Some(FieldKind::Enum),
            "url" => Some(FieldKind::Url),
            "ref" => Some(FieldKind::Ref),
            "image" => Some(FieldKind::Image),
            _ => None,
        }
    }

    pub fn contains_ref(&self) -> bool {
        match self {
            FieldKind::Ref => true,
            FieldKind::List(inner) => inner.contains_ref(),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 단순_kind_파싱() {
        assert_eq!(FieldKind::parse("text"), Some(FieldKind::Text));
        assert_eq!(FieldKind::parse("richtext"), Some(FieldKind::RichText));
        assert_eq!(FieldKind::parse("money"), Some(FieldKind::Money));
        assert_eq!(FieldKind::parse("ref"), Some(FieldKind::Ref));
    }

    #[test]
    fn 리스트_kind_파싱() {
        assert_eq!(
            FieldKind::parse("list<ref>"),
            Some(FieldKind::List(Box::new(FieldKind::Ref)))
        );
        assert_eq!(
            FieldKind::parse("list<text>"),
            Some(FieldKind::List(Box::new(FieldKind::Text)))
        );
    }

    #[test]
    fn 잘못된_kind는_none() {
        assert_eq!(FieldKind::parse("geo"), None);
        assert_eq!(FieldKind::parse("list<list<text>>"), None); // 중첩 리스트 금지
        assert_eq!(FieldKind::parse(""), None);
    }

    #[test]
    fn contains_ref_판정() {
        assert!(FieldKind::Ref.contains_ref());
        assert!(FieldKind::parse("list<ref>").unwrap().contains_ref());
        assert!(!FieldKind::Text.contains_ref());
    }
}
