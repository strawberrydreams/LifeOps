use crate::import::map::Source;
use crate::import::{canonicalize_hangul, hangul_eq, ImportError};
use serde_json::{Map, Value};

#[derive(Debug)]
pub struct RuleSet {
    pub rules: Vec<Rule>,
}

#[derive(Debug)]
pub struct Rule {
    pub matcher: Matcher,
    pub action: Action,
    pub provenance: String,
    pub is_default: bool,
    pub to: Option<String>,
}

#[derive(Debug, Default)]
pub struct Matcher {
    pub fm: Vec<(String, Vec<String>)>,
    pub path_glob: Option<String>,
    pub always: bool,
}

#[derive(Debug)]
pub enum Action {
    Skip,
    Single {
        map: Vec<(String, Source)>,
    },
    Table {
        cols: Vec<(Vec<String>, String)>,
        set: Vec<(String, Source)>,
    },
}

impl RuleSet {
    pub fn from_yaml(source: &str) -> Result<Self, ImportError> {
        let raw: RawRoot = serde_yaml::from_str(source)
            .map_err(|error| rule_error(0, format!("YAML 파싱 실패: {error}")))?;
        let rule_count = raw.rules.len();
        let mut default_index = None;
        let mut rules = Vec::with_capacity(rule_count);

        for (index, raw_rule) in raw.rules.into_iter().enumerate() {
            let (rule, is_default) = convert_rule(raw_rule, index)?;
            if is_default {
                if default_index.is_some() {
                    return Err(rule_error(index, "default 규칙은 정확히 하나여야 함"));
                }
                if index + 1 != rule_count {
                    return Err(rule_error(index, "default 규칙은 마지막이어야 함"));
                }
                default_index = Some(index);
            }
            rules.push(rule);
        }

        if default_index.is_none() {
            return Err(rule_error(
                rule_count.saturating_sub(1),
                "default 규칙이 정확히 하나 필요함",
            ));
        }

        Ok(Self { rules })
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRoot {
    rules: Vec<serde_yaml::Value>,
}

fn convert_rule(raw: serde_yaml::Value, index: usize) -> Result<(Rule, bool), ImportError> {
    let mapping = raw
        .as_mapping()
        .ok_or_else(|| rule_error(index, "규칙은 매핑이어야 함"))?;
    validate_keys(
        mapping,
        &[
            "match",
            "default",
            "to",
            "skip",
            "map",
            "rows",
            "cols",
            "set",
            "provenance",
        ],
        index,
    )?;

    let has_match = has_key(mapping, "match");
    let has_default = has_key(mapping, "default");
    if has_match == has_default {
        return Err(rule_error(index, "match와 default 중 정확히 하나가 필요함"));
    }

    let (matcher, action_mapping, is_default) = if has_default {
        if mapping.len() != 1 {
            return Err(rule_error(
                index,
                "default 액션 필드는 default wrapper 안에 있어야 함",
            ));
        }
        let action_mapping = get(mapping, "default")
            .and_then(serde_yaml::Value::as_mapping)
            .ok_or_else(|| rule_error(index, "default는 액션 매핑이어야 함"))?;
        (
            Matcher {
                always: true,
                ..Matcher::default()
            },
            action_mapping,
            true,
        )
    } else {
        let matcher = parse_matcher(get(mapping, "match").expect("has_match로 존재 확인"), index)?;
        (matcher, mapping, false)
    };

    let action_keys = if is_default {
        &["to", "skip", "map", "rows", "cols", "set", "provenance"][..]
    } else {
        &[
            "match",
            "to",
            "skip",
            "map",
            "rows",
            "cols",
            "set",
            "provenance",
        ][..]
    };
    validate_keys(action_mapping, action_keys, index)?;
    let (action, to) = parse_action(action_mapping, index)?;
    let provenance = match get(action_mapping, "provenance") {
        Some(value) => required_nonempty_string(value, "provenance", index)?,
        None => "imported".to_string(),
    };

    Ok((
        Rule {
            matcher,
            action,
            provenance,
            is_default,
            to,
        },
        is_default,
    ))
}

fn parse_matcher(value: &serde_yaml::Value, index: usize) -> Result<Matcher, ImportError> {
    let map = value
        .as_mapping()
        .ok_or_else(|| rule_error(index, "match는 매핑이어야 함"))?;
    if map.is_empty() {
        return Err(rule_error(index, "match는 비어 있을 수 없음"));
    }
    let mut matcher = Matcher::default();

    for (key, value) in map {
        let key = required_nonempty_string(key, "match 키", index)?;
        if key == "path" {
            matcher.path_glob = Some(required_nonempty_string(value, "path", index)?);
        } else if let Some(frontmatter_key) = key.strip_prefix("fm.") {
            if frontmatter_key.trim().is_empty() {
                return Err(rule_error(index, "fm match 키가 비어 있음"));
            }
            let allowed = match value {
                serde_yaml::Value::Sequence(values) => {
                    if values.is_empty() {
                        return Err(rule_error(
                            index,
                            format!("fm.{frontmatter_key} 허용값이 비어 있음"),
                        ));
                    }
                    values
                        .iter()
                        .map(|value| fm_scalar(value, index))
                        .collect::<Result<_, _>>()?
                }
                value => vec![fm_scalar(value, index)?],
            };
            matcher.fm.push((frontmatter_key.to_string(), allowed));
        } else {
            return Err(rule_error(index, format!("알 수 없는 match 키 '{key}'")));
        }
    }

    Ok(matcher)
}

fn parse_action(
    mapping: &serde_yaml::Mapping,
    index: usize,
) -> Result<(Action, Option<String>), ImportError> {
    if let Some(skip) = get(mapping, "skip") {
        if skip != &serde_yaml::Value::Bool(true) {
            return Err(rule_error(index, "skip은 true여야 함"));
        }
        if ["to", "map", "rows", "cols", "set"]
            .iter()
            .any(|key| has_key(mapping, key))
        {
            return Err(rule_error(index, "skip은 다른 액션 필드와 함께 쓸 수 없음"));
        }
        return Ok((Action::Skip, None));
    }

    let to = get(mapping, "to")
        .ok_or_else(|| rule_error(index, "액션에 to가 필요함"))
        .and_then(|value| required_nonempty_string(value, "to", index))?;
    let has_map = has_key(mapping, "map");
    let has_table_field = ["rows", "cols", "set"]
        .iter()
        .any(|key| has_key(mapping, key));

    if has_map && has_table_field {
        return Err(rule_error(index, "map 액션과 table 액션을 함께 쓸 수 없음"));
    }

    if has_map {
        let map = source_mapping(
            get(mapping, "map").expect("has_map으로 존재 확인"),
            "map",
            true,
            index,
        )?;
        return Ok((Action::Single { map }, Some(to)));
    }

    if has_table_field {
        let rows =
            get(mapping, "rows").ok_or_else(|| rule_error(index, "table 액션에 rows가 필요함"))?;
        if required_nonempty_string(rows, "rows", index)? != "table" {
            return Err(rule_error(index, "rows는 정확히 'table'이어야 함"));
        }
        let cols_value =
            get(mapping, "cols").ok_or_else(|| rule_error(index, "table 액션에 cols가 필요함"))?;
        let cols = cols_mapping(cols_value, index)?;
        let set = match get(mapping, "set") {
            Some(value) => source_mapping(value, "set", false, index)?,
            None => Vec::new(),
        };
        return Ok((Action::Table { cols, set }, Some(to)));
    }

    Err(rule_error(index, "single 액션에 map이 필요함"))
}

fn source_mapping(
    value: &serde_yaml::Value,
    name: &str,
    require_nonempty: bool,
    index: usize,
) -> Result<Vec<(String, Source)>, ImportError> {
    let mapping = value
        .as_mapping()
        .ok_or_else(|| rule_error(index, format!("{name}은 매핑이어야 함")))?;
    if require_nonempty && mapping.is_empty() {
        return Err(rule_error(index, format!("{name}은 비어 있을 수 없음")));
    }
    mapping
        .iter()
        .map(|(field, source)| {
            let field = required_nonempty_string(field, &format!("{name} field"), index)?;
            let source = required_nonempty_string(source, &format!("{name} source"), index)?;
            Ok((field, Source::parse(&source)))
        })
        .collect()
}

fn cols_mapping(
    value: &serde_yaml::Value,
    index: usize,
) -> Result<Vec<(Vec<String>, String)>, ImportError> {
    let mapping = value
        .as_mapping()
        .ok_or_else(|| rule_error(index, "cols는 매핑이어야 함"))?;
    if mapping.is_empty() {
        return Err(rule_error(index, "cols는 비어 있을 수 없음"));
    }
    mapping
        .iter()
        .map(|(headers, target)| {
            let headers = required_nonempty_string(headers, "cols header", index)?;
            let headers = headers
                .split('|')
                .map(str::trim)
                .map(|header| {
                    if header.is_empty() {
                        Err(rule_error(index, "cols header 후보가 비어 있음"))
                    } else {
                        Ok(header.to_string())
                    }
                })
                .collect::<Result<_, _>>()?;
            let target = required_nonempty_string(target, "cols target", index)?;
            Ok((headers, target))
        })
        .collect()
}

fn fm_scalar(value: &serde_yaml::Value, index: usize) -> Result<String, ImportError> {
    match value {
        serde_yaml::Value::String(string) if !string.trim().is_empty() => Ok(string.clone()),
        serde_yaml::Value::Bool(boolean) => Ok(boolean.to_string()),
        serde_yaml::Value::Number(number) => Ok(number.to_string()),
        serde_yaml::Value::String(_) => Err(rule_error(index, "fm 허용 문자열이 비어 있음")),
        _ => Err(rule_error(
            index,
            "fm 허용값은 string/bool/number scalar 또는 이들의 배열이어야 함",
        )),
    }
}

fn required_nonempty_string(
    value: &serde_yaml::Value,
    name: &str,
    index: usize,
) -> Result<String, ImportError> {
    match value {
        serde_yaml::Value::String(string) if !string.trim().is_empty() => Ok(string.clone()),
        serde_yaml::Value::String(_) => Err(rule_error(index, format!("{name}이 비어 있음"))),
        _ => Err(rule_error(index, format!("{name}은 문자열이어야 함"))),
    }
}

fn validate_keys(
    mapping: &serde_yaml::Mapping,
    allowed: &[&str],
    index: usize,
) -> Result<(), ImportError> {
    for key in mapping.keys() {
        let key = required_nonempty_string(key, "규칙 키", index)?;
        if !allowed.contains(&key.as_str()) {
            return Err(rule_error(index, format!("알 수 없는 규칙 키 '{key}'")));
        }
    }
    Ok(())
}

fn has_key(mapping: &serde_yaml::Mapping, key: &str) -> bool {
    get(mapping, key).is_some()
}

fn get<'a>(mapping: &'a serde_yaml::Mapping, key: &str) -> Option<&'a serde_yaml::Value> {
    mapping.get(serde_yaml::Value::String(key.to_string()))
}

fn rule_error(index: usize, message: impl Into<String>) -> ImportError {
    ImportError::Rules(format!("rules[{index}]: {}", message.into()))
}

/// fm 조건은 모두 AND로, 각 허용값은 실제 scalar/array 값 중 하나와 비교한다.
/// path 조건이 있으면 `*` glob도 함께 만족해야 한다.
pub fn matches(matcher: &Matcher, relpath: &str, frontmatter: &Map<String, Value>) -> bool {
    if matcher.always {
        return true;
    }

    for (key, allowed) in &matcher.fm {
        let Some(actual) = frontmatter.get(key) else {
            return false;
        };
        let hit = match actual {
            Value::String(actual) => allowed.iter().any(|allowed| hangul_eq(allowed, actual)),
            Value::Array(items) => items.iter().any(|actual| match actual {
                Value::String(actual) => allowed.iter().any(|allowed| hangul_eq(allowed, actual)),
                Value::Bool(_) | Value::Number(_) => {
                    let actual = actual.to_string();
                    allowed.iter().any(|allowed| allowed == &actual)
                }
                _ => false,
            }),
            Value::Bool(_) | Value::Number(_) => {
                let actual = actual.to_string();
                allowed.iter().any(|allowed| allowed == &actual)
            }
            Value::Null | Value::Object(_) => false,
        };
        if !hit {
            return false;
        }
    }

    matcher
        .path_glob
        .as_deref()
        .is_none_or(|pattern| glob_match(pattern, relpath))
}

/// `*` 와일드카드만 지원하는 단순 glob. `*`는 `/`도 포함해 매치한다.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let pattern = canonicalize_hangul(pattern);
    let text = canonicalize_hangul(text);
    glob_match_canonical(&pattern, &text)
}

fn glob_match_canonical(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == text;
    }

    let mut offset = 0;
    let first = parts.first().expect("split은 적어도 한 조각을 반환");
    if !text.starts_with(first) {
        return false;
    }
    offset += first.len();

    for middle in &parts[1..parts.len() - 1] {
        if middle.is_empty() {
            continue;
        }
        let Some(position) = text[offset..].find(middle) else {
            return false;
        };
        offset += position + middle.len();
    }

    text[offset..].ends_with(parts.last().expect("split은 적어도 한 조각을 반환"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const YAML: &str = r#"
rules:
  - match: { fm.source: x }
    to: 노트
    map:
      제목: fm.aliases[0] | fm.title | filename
      본문: body
      출처: fm.handle | fm.author
      url: fm.url
    provenance: imported:x
  - match: { path: "*위시리스트*" }
    to: 물건
    rows: table
    cols:
      "제품|분류": 이름
      가격: 가격
      링크: 구매링크
    set:
      상태: "위시"
      카테고리: dirs
    provenance: imported
  - match: { fm.type: [moc, tag-map, dashboard] }
    skip: true
  - default:
      to: 노트
      map:
        제목: fm.title | filename
        본문: body
      provenance: imported
"#;

    fn with_default(rule: &str) -> String {
        format!(
            "rules:\n{rule}  - default:\n      to: 노트\n      map: {{ 제목: filename }}\n      provenance: imported\n"
        )
    }

    fn assert_invalid(source: &str, rule_index: usize) {
        let error = RuleSet::from_yaml(source).unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains(&format!("rules[{rule_index}]")),
            "규칙 인덱스가 없는 오류: {message}"
        );
    }

    #[test]
    fn 규칙셋을_파싱한다() {
        let rs = RuleSet::from_yaml(YAML).unwrap();
        assert_eq!(rs.rules.len(), 4);
        assert!(matches!(rs.rules[0].action, Action::Single { .. }));
        assert!(matches!(rs.rules[1].action, Action::Table { .. }));
        assert!(matches!(rs.rules[2].action, Action::Skip));
        assert!(rs.rules[3].is_default);
        assert_eq!(rs.rules[0].provenance, "imported:x");
        let Action::Single { map } = &rs.rules[0].action else {
            panic!("첫 규칙은 단일 엔티티 액션이어야 함");
        };
        assert_eq!(
            map.iter()
                .map(|(field, _)| field.as_str())
                .collect::<Vec<_>>(),
            ["제목", "본문", "출처", "url"]
        );
        let Action::Table { cols, set } = &rs.rules[1].action else {
            panic!("둘째 규칙은 표 액션이어야 함");
        };
        assert_eq!(
            cols.iter()
                .map(|(headers, field)| (headers.clone(), field.clone()))
                .collect::<Vec<_>>(),
            vec![
                (
                    vec!["제품".to_string(), "분류".to_string()],
                    "이름".to_string()
                ),
                (vec!["가격".to_string()], "가격".to_string()),
                (vec!["링크".to_string()], "구매링크".to_string()),
            ]
        );
        assert_eq!(
            set.iter()
                .map(|(field, _)| field.as_str())
                .collect::<Vec<_>>(),
            ["상태", "카테고리"]
        );
        assert_eq!(rs.rules[3].to.as_deref(), Some("노트"));
        assert_eq!(rs.rules[3].provenance, "imported");
        let Action::Single { map } = &rs.rules[3].action else {
            panic!("default wrapper의 map은 단일 엔티티 액션이어야 함");
        };
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn fm_동등과_리스트_멤버십_매치() {
        let rs = RuleSet::from_yaml(YAML).unwrap();
        let x_fm = json!({ "source": "x" }).as_object().unwrap().clone();
        assert!(matches(&rs.rules[0].matcher, "any.md", &x_fm));
        let moc_fm = json!({ "type": "moc" }).as_object().unwrap().clone();
        assert!(matches(&rs.rules[2].matcher, "any.md", &moc_fm));
        let other = json!({ "type": "reference" }).as_object().unwrap().clone();
        assert!(!matches(&rs.rules[2].matcher, "any.md", &other));

        let nfd_fm = json!({ "type": "대시보드" }).as_object().unwrap().clone();
        let nfc_rules = RuleSet::from_yaml(&with_default(
            "  - match: { fm.type: [대시보드] }\n    skip: true\n",
        ))
        .unwrap();
        assert!(matches(&nfc_rules.rules[0].matcher, "any.md", &nfd_fm));

        let scalar_rules = RuleSet::from_yaml(&with_default(
            "  - match: { fm.published: true, fm.rank: [1, 2] }\n    skip: true\n",
        ))
        .unwrap();
        let scalar_fm = json!({ "published": true, "rank": [0, 2] })
            .as_object()
            .unwrap()
            .clone();
        assert!(matches(
            &scalar_rules.rules[0].matcher,
            "any.md",
            &scalar_fm
        ));

        let structured_rules = RuleSet::from_yaml(&with_default(
            "  - match: { fm.kind: ['null', '{\"nested\":\"x\"}'] }\n    skip: true\n",
        ))
        .unwrap();
        for structured_fm in [
            json!({ "kind": null }),
            json!({ "kind": { "nested": "x" } }),
        ] {
            assert!(!matches(
                &structured_rules.rules[0].matcher,
                "any.md",
                structured_fm.as_object().unwrap()
            ));
        }
    }

    #[test]
    fn path_glob_매치() {
        assert!(glob_match(
            "*위시리스트*",
            "OneDrive/럭셔리/시계/시계 위시리스트.md"
        ));
        assert!(glob_match(
            "*/_maps/*",
            "Obsidian/Clara/Archives/_maps/x.md"
        ));
        assert!(!glob_match("*위시리스트*", "Knowleage/Linux/권한.md"));
        assert!(glob_match(
            "*위시리스트*",
            "OneDrive/럭셔리/시계/시계 위시리스트.md"
        ));
    }

    #[test]
    fn default는_술어없이_항상_매치() {
        let rs = RuleSet::from_yaml(YAML).unwrap();
        let empty = serde_json::Map::new();
        assert!(matches(&rs.rules[3].matcher, "무엇이든.md", &empty));
    }

    #[test]
    fn matcher와_default_구조를_엄격히_검증한다() {
        let cases = [
            with_default("  - to: 노트\n    map: { 제목: filename }\n"),
            with_default(
                "  - match: { fm.type: note }\n    default:\n      to: 노트\n      map: { 제목: filename }\n    to: 노트\n    map: { 제목: filename }\n",
            ),
            with_default("  - match: {}\n    to: 노트\n    map: { 제목: filename }\n"),
            with_default(
                "  - macth: { fm.type: note }\n    to: 노트\n    map: { 제목: filename }\n",
            ),
            "rules:\n  - default:\n      to: 노트\n      map: { 제목: filename }\n  - match: { fm.type: note }\n    to: 노트\n    map: { 제목: filename }\n"
                .to_string(),
            "rules:\n  - default:\n      to: 노트\n      map: { 제목: filename }\n  - default:\n      to: 노트\n      map: { 제목: filename }\n"
                .to_string(),
            "rules:\n  - match: { fm.type: note }\n    to: 노트\n    map: { 제목: filename }\n"
                .to_string(),
            "rules:\n  - default:\n      match: { fm.type: note }\n      skip: true\n".to_string(),
        ];

        for source in &cases[..4] {
            assert_invalid(source, 0);
        }
        assert_invalid(&cases[4], 0);
        assert_invalid(&cases[5], 0);
        assert_invalid(&cases[6], 0);
        assert_invalid(&cases[7], 0);
    }

    #[test]
    fn action_조합을_엄격히_검증한다() {
        let cases = [
            with_default(
                "  - match: { fm.type: note }\n    skip: true\n    to: 노트\n    map: { 제목: filename }\n",
            ),
            with_default("  - match: { fm.type: note }\n    to: 노트\n    map: {}\n"),
            with_default("  - match: { fm.type: note }\n    map: { 제목: filename }\n"),
            with_default(
                "  - match: { fm.type: note }\n    rows: table\n    cols: { 제품: 이름 }\n",
            ),
            with_default(
                "  - match: { fm.type: note }\n    to: 물건\n    rows: tables\n    cols: { 제품: 이름 }\n",
            ),
            with_default("  - match: { fm.type: note }\n    to: 물건\n    rows: table\n"),
            with_default(
                "  - match: { fm.type: note }\n    to: 물건\n    rows: table\n    cols: { 제품: 이름 }\n    map: { 이름: filename }\n",
            ),
        ];

        for source in &cases {
            assert_invalid(source, 0);
        }

        assert_invalid(
            "rules:\n  - match: { fm.type: first }\n    skip: true\n  - match: { fm.type: second }\n    to: 물건\n    rows: tables\n    cols: { 제품: 이름 }\n  - default:\n      to: 노트\n      map: { 제목: filename }\n",
            1,
        );
    }

    #[test]
    fn context별_scalar와_빈_문자열을_거부한다() {
        let cases = [
            with_default("  - match: { path: [x] }\n    skip: true\n"),
            with_default("  - match: { path: \"\" }\n    skip: true\n"),
            with_default("  - match: { fm.type: { nested: x } }\n    skip: true\n"),
            with_default("  - match: { fm.type: [x, [y]] }\n    skip: true\n"),
            with_default("  - match: { fm.type: null }\n    skip: true\n"),
            with_default("  - match: { fm.type: x }\n    to: \"\"\n    map: { 제목: filename }\n"),
            with_default("  - match: { fm.type: x }\n    to: 노트\n    map: { 제목: [filename] }\n"),
            with_default("  - match: { fm.type: x }\n    to: 노트\n    map: { \"\": filename }\n"),
            with_default("  - match: { fm.type: x }\n    to: 노트\n    map: { 제목: \"\" }\n"),
            with_default(
                "  - match: { fm.type: x }\n    to: 물건\n    rows: table\n    cols: { \"|\": 이름 }\n",
            ),
            with_default(
                "  - match: { fm.type: x }\n    to: 물건\n    rows: table\n    cols: { 제품: \"\" }\n",
            ),
            with_default(
                "  - match: { fm.type: x }\n    to: 물건\n    rows: table\n    cols: { 제품: 이름 }\n    set: { 상태: [위시] }\n",
            ),
            with_default(
                "  - match: { fm.type: x }\n    to: 노트\n    map: { 제목: filename }\n    provenance: \"\"\n",
            ),
        ];

        for source in &cases {
            assert_invalid(source, 0);
        }
    }
}
