use serde_json::{Map, Value};

/// 파일 하나의 매핑 표현식을 평가하는 컨텍스트.
pub struct FileCtx<'a> {
    pub frontmatter: &'a Map<String, Value>,
    pub body: &'a str,
    pub filename: &'a str,
    /// 파일의 조상 디렉터리명. 최심부 우선으로 전달된다.
    pub dirs: &'a [String],
}

#[derive(Debug, Clone)]
enum Atom {
    Fm(String, Option<usize>),
    Body,
    Filename,
    Dirs,
    Literal(String),
    Invalid,
}

/// `a | b | c`처럼 왼쪽부터 첫 비어 있지 않은 원자 값을 선택하는 소스 표현식.
#[derive(Debug, Clone)]
pub struct Source(Vec<Atom>);

impl Source {
    /// 소스 표현식을 파싱한다.
    ///
    /// 알 수 없는 토큰은 규칙 파일을 관대하게 읽기 위해 문자열 리터럴로 취급한다.
    pub fn parse(expr: &str) -> Source {
        let atoms = expr
            .split('|')
            .map(|part| parse_atom(part.trim()))
            .collect();
        Source(atoms)
    }

    /// 표현식을 평가해 첫 비어 있지 않은 값을 반환한다.
    pub fn eval(&self, ctx: &FileCtx<'_>) -> Option<Value> {
        self.0
            .iter()
            .filter_map(|atom| eval_atom(atom, ctx))
            .find(|value| !is_empty(value))
    }
}

fn parse_atom(source: &str) -> Atom {
    if let Some(literal) = source
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
    {
        return Atom::Literal(literal.to_string());
    }

    match source {
        "body" => Atom::Body,
        "filename" => Atom::Filename,
        "dirs" => Atom::Dirs,
        _ => {
            if let Some(key) = source.strip_prefix("fm.") {
                return parse_frontmatter_atom(key);
            }

            Atom::Literal(source.to_string())
        }
    }
}

fn parse_frontmatter_atom(key: &str) -> Atom {
    let Some(index_start) = key.find('[') else {
        return if key.contains(']') {
            Atom::Invalid
        } else {
            Atom::Fm(key.to_string(), None)
        };
    };

    let name = &key[..index_start];
    let index = key[index_start..]
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .filter(|index| {
            !index.is_empty() && index.bytes().all(|character| character.is_ascii_digit())
        })
        .and_then(|index| index.parse::<usize>().ok());

    if name.is_empty() || name.contains(']') {
        return Atom::Invalid;
    }

    match index {
        Some(index) => Atom::Fm(name.to_string(), Some(index)),
        None => Atom::Invalid,
    }
}

fn eval_atom(atom: &Atom, ctx: &FileCtx<'_>) -> Option<Value> {
    match atom {
        Atom::Fm(key, index) => {
            let value = ctx.frontmatter.get(key)?;
            match index {
                Some(index) => value.as_array()?.get(*index).cloned(),
                None => Some(value.clone()),
            }
        }
        Atom::Body => Some(Value::String(ctx.body.to_string())),
        Atom::Filename => Some(Value::String(ctx.filename.to_string())),
        Atom::Dirs => Some(Value::Array(
            ctx.dirs.iter().cloned().map(Value::String).collect(),
        )),
        Atom::Literal(literal) => Some(Value::String(literal.clone())),
        Atom::Invalid => None,
    }
}

fn is_empty(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(string) => string.is_empty(),
        Value::Array(items) => items.is_empty(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx<'a>(
        fm: &'a Map<String, Value>,
        body: &'a str,
        filename: &'a str,
        dirs: &'a [String],
    ) -> FileCtx<'a> {
        FileCtx {
            frontmatter: fm,
            body,
            filename,
            dirs,
        }
    }

    #[test]
    fn fm_와_인덱스_평가() {
        let fm: Map<String, Value> = json!({
            "title": "권한",
            "aliases": ["별칭1", "별칭2"]
        })
        .as_object()
        .unwrap()
        .clone();
        let dirs: Vec<String> = vec![];

        assert_eq!(
            Source::parse("fm.title").eval(&ctx(&fm, "b", "f", &dirs)),
            Some(json!("권한"))
        );
        assert_eq!(
            Source::parse("fm.aliases[0]").eval(&ctx(&fm, "b", "f", &dirs)),
            Some(json!("별칭1"))
        );
        assert_eq!(
            Source::parse("fm.missing").eval(&ctx(&fm, "b", "f", &dirs)),
            None
        );
    }

    #[test]
    fn 잘못되거나_평가할_수_없는_fm_인덱스는_폴백한다() {
        let fm: Map<String, Value> = json!({
            "aliases": ["별칭1", "별칭2"],
            "title": "권한"
        })
        .as_object()
        .unwrap()
        .clone();
        let dirs: Vec<String> = vec![];
        let context = ctx(&fm, "b", "폴백", &dirs);

        for malformed in [
            "fm.aliases[x]",
            "fm.aliases[]",
            "fm.aliases[0]junk",
            "fm.aliases[0][1]",
        ] {
            assert_eq!(
                Source::parse(&format!("{malformed} | filename")).eval(&context),
                Some(json!("폴백")),
                "잘못된 인덱스 표현식: {malformed}"
            );
        }

        for unavailable in ["fm.aliases[2]", "fm.title[0]"] {
            assert_eq!(
                Source::parse(&format!("{unavailable} | filename")).eval(&context),
                Some(json!("폴백")),
                "평가할 수 없는 인덱스 표현식: {unavailable}"
            );
        }
    }

    #[test]
    fn body_filename_dirs_리터럴() {
        let fm = Map::new();
        let dirs = vec!["수집품".to_string(), "럭셔리".to_string()];

        assert_eq!(
            Source::parse("body").eval(&ctx(&fm, "본문내용", "파일", &dirs)),
            Some(json!("본문내용"))
        );
        assert_eq!(
            Source::parse("filename").eval(&ctx(&fm, "b", "제목없음", &dirs)),
            Some(json!("제목없음"))
        );
        assert_eq!(
            Source::parse("dirs").eval(&ctx(&fm, "b", "f", &dirs)),
            Some(json!(["수집품", "럭셔리"]))
        );
        assert_eq!(
            Source::parse("\"위시\"").eval(&ctx(&fm, "b", "f", &dirs)),
            Some(json!("위시"))
        );
    }

    #[test]
    fn first_of_는_첫_비어있지않은_값() {
        let fm: Map<String, Value> = json!({
            "title": "",
            "author": "@handle",
            "nothing": null,
            "empty": []
        })
        .as_object()
        .unwrap()
        .clone();
        let dirs: Vec<String> = vec![];

        assert_eq!(
            Source::parse("fm.title | fm.author | filename").eval(&ctx(&fm, "b", "폴백", &dirs)),
            Some(json!("@handle"))
        );
        assert_eq!(
            Source::parse("fm.x | fm.y | filename").eval(&ctx(&fm, "b", "폴백", &dirs)),
            Some(json!("폴백"))
        );
        assert_eq!(
            Source::parse("fm.nothing | fm.empty | unknown").eval(&ctx(&fm, "b", "폴백", &dirs)),
            Some(json!("unknown"))
        );
    }
}
