//! archives Markdown → 엔티티/노트 임포터 (계획 10).
//! 순수 로직만 — 파일시스템/인자 파싱은 lifeops-cli가 담당한다.

pub mod apply;
pub mod coerce;
pub mod map;
pub mod markdown;
pub mod route;
pub mod rules;
pub mod table;

pub use apply::{apply, plan, ApplyOpts, FileInput, ImportPlan, ImportReport, PlanStats};
pub use coerce::{coerce, extract_url, parse_price_krw};
pub use map::{FileCtx, Source};
pub use markdown::{parse_document, ParsedDoc};
pub use route::{route_file, validate_rules_schema, PlannedEntity, RouteResult};
pub use rules::{glob_match, matches, Action, Matcher, Rule, RuleSet};
pub use table::{parse_tables, Table};

#[derive(Debug)]
pub enum ImportError {
    Rules(String),
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImportError::Rules(message) => write!(f, "규칙 파싱 실패: {message}"),
        }
    }
}

impl std::error::Error for ImportError {}

/// 현대 한글의 정규 분해 자모를 완성형 음절로 조합한다.
///
/// 외부 Unicode 정규화 의존성 없이 macOS 경로의 NFD 한글과 스키마의 NFC 한글을
/// 비교하기 위한 좁은 용도다. 한글 조합 대상이 아닌 문자는 그대로 보존한다.
pub(crate) fn canonicalize_hangul(input: &str) -> String {
    const S_BASE: u32 = 0xAC00;
    const L_BASE: u32 = 0x1100;
    const V_BASE: u32 = 0x1161;
    const T_BASE: u32 = 0x11A7;
    const L_COUNT: u32 = 19;
    const V_COUNT: u32 = 21;
    const T_COUNT: u32 = 28;

    let mut chars = input.chars().peekable();
    let mut result = String::with_capacity(input.len());

    while let Some(character) = chars.next() {
        let code = character as u32;

        if (L_BASE..L_BASE + L_COUNT).contains(&code) {
            if let Some(&vowel) = chars.peek() {
                let vowel_code = vowel as u32;
                if (V_BASE..V_BASE + V_COUNT).contains(&vowel_code) {
                    chars.next();
                    let mut syllable =
                        S_BASE + ((code - L_BASE) * V_COUNT + (vowel_code - V_BASE)) * T_COUNT;

                    if let Some(&trailing) = chars.peek() {
                        let trailing_code = trailing as u32;
                        if (T_BASE + 1..T_BASE + T_COUNT).contains(&trailing_code) {
                            chars.next();
                            syllable += trailing_code - T_BASE;
                        }
                    }

                    result.push(char::from_u32(syllable).expect("한글 음절 범위"));
                    continue;
                }
            }
        }

        // 이미 조합된 받침 없는 음절 뒤에 정규 종성 자모가 오는 경우도 조합한다.
        if (S_BASE..S_BASE + L_COUNT * V_COUNT * T_COUNT).contains(&code)
            && (code - S_BASE).is_multiple_of(T_COUNT)
        {
            if let Some(&trailing) = chars.peek() {
                let trailing_code = trailing as u32;
                if (T_BASE + 1..T_BASE + T_COUNT).contains(&trailing_code) {
                    chars.next();
                    let syllable = code + trailing_code - T_BASE;
                    result.push(char::from_u32(syllable).expect("한글 음절 범위"));
                    continue;
                }
            }
        }

        result.push(character);
    }

    result
}

/// 현대 한글의 NFC/NFD 표현 차이를 무시하고 두 문자열을 비교한다.
pub(crate) fn hangul_eq(left: &str, right: &str) -> bool {
    canonicalize_hangul(left) == canonicalize_hangul(right)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 현대_한글_자모를_조합하고_비한글은_보존한다() {
        assert_eq!(
            canonicalize_hangul("OneDrive/전자제품.md"),
            "OneDrive/전자제품.md"
        );
        assert!(hangul_eq("전자제품", "전자제품"));
        assert_eq!(canonicalize_hangul("각"), "각", "L+V+T 조합");
        assert_eq!(canonicalize_hangul("각"), "각", "LV+T 조합");
        assert_eq!(canonicalize_hangul("ᄀ-x-ᆨ"), "ᄀ-x-ᆨ", "고립 자모 보존");
    }
}
