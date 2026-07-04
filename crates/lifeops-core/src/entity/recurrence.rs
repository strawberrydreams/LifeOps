use chrono::{Datelike, Duration, NaiveDate, Weekday};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecurrenceRule {
    Daily,
    Weekly(Option<Weekday>),
    Monthly(Option<u32>),
    Yearly(Option<(u32, u32)>),
}

pub fn parse_rule(s: &str) -> Option<RecurrenceRule> {
    let s = s.trim();
    if s == "매일" {
        return Some(RecurrenceRule::Daily);
    }
    if let Some(rest) = s.strip_prefix("매주") {
        let rest = rest.trim();
        if rest.is_empty() {
            return Some(RecurrenceRule::Weekly(None));
        }
        return weekday_ko(rest).map(|w| RecurrenceRule::Weekly(Some(w)));
    }
    if let Some(rest) = s.strip_prefix("매월") {
        let rest = rest.trim();
        if rest.is_empty() {
            return Some(RecurrenceRule::Monthly(None));
        }
        let day: u32 = rest.strip_suffix('일')?.trim().parse().ok()?;
        if !(1..=31).contains(&day) {
            return None;
        }
        return Some(RecurrenceRule::Monthly(Some(day)));
    }
    if let Some(rest) = s.strip_prefix("매년") {
        let rest = rest.trim();
        if rest.is_empty() {
            return Some(RecurrenceRule::Yearly(None));
        }
        let (m, d) = rest.split_once('-')?;
        let m: u32 = m.trim().parse().ok()?;
        let d: u32 = d.trim().parse().ok()?;
        if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
            return None;
        }
        return Some(RecurrenceRule::Yearly(Some((m, d))));
    }
    None
}

fn weekday_ko(s: &str) -> Option<Weekday> {
    match s {
        "월" => Some(Weekday::Mon),
        "화" => Some(Weekday::Tue),
        "수" => Some(Weekday::Wed),
        "목" => Some(Weekday::Thu),
        "금" => Some(Weekday::Fri),
        "토" => Some(Weekday::Sat),
        "일" => Some(Weekday::Sun),
        _ => None,
    }
}

/// base에서 규칙 단위로 전진, 결과가 오늘 이하이면 오늘을 넘길 때까지 반복.
pub fn next_date(rule: &RecurrenceRule, base: NaiveDate, today: NaiveDate) -> NaiveDate {
    let mut next = step(rule, base);
    while next <= today {
        next = step(rule, next);
    }
    next
}

fn step(rule: &RecurrenceRule, from: NaiveDate) -> NaiveDate {
    match rule {
        RecurrenceRule::Daily => from + Duration::days(1),
        RecurrenceRule::Weekly(None) => from + Duration::days(7),
        RecurrenceRule::Weekly(Some(w)) => {
            let mut d = from + Duration::days(1);
            while d.weekday() != *w {
                d += Duration::days(1);
            }
            d
        }
        RecurrenceRule::Monthly(day) => {
            let target = day.unwrap_or(from.day());
            let (y, m) = if from.month() == 12 {
                (from.year() + 1, 1)
            } else {
                (from.year(), from.month() + 1)
            };
            clamped(y, m, target)
        }
        RecurrenceRule::Yearly(md) => {
            let (m, d) = md.unwrap_or((from.month(), from.day()));
            clamped(from.year() + 1, m, d)
        }
    }
}

/// 해당 월에 없는 일자는 말일로 클램프.
fn clamped(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap_or_else(|| {
        let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
        NaiveDate::from_ymd_opt(ny, nm, 1).expect("유효한 다음 달 1일") - Duration::days(1)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn 문법_4종_파싱() {
        assert_eq!(parse_rule("매일"), Some(RecurrenceRule::Daily));
        assert_eq!(parse_rule("매주"), Some(RecurrenceRule::Weekly(None)));
        assert_eq!(parse_rule("매주 월"), Some(RecurrenceRule::Weekly(Some(chrono::Weekday::Mon))));
        assert_eq!(parse_rule("매월"), Some(RecurrenceRule::Monthly(None)));
        assert_eq!(parse_rule("매월 15일"), Some(RecurrenceRule::Monthly(Some(15))));
        assert_eq!(parse_rule("매년"), Some(RecurrenceRule::Yearly(None)));
        assert_eq!(parse_rule("매년 3-14"), Some(RecurrenceRule::Yearly(Some((3, 14)))));
        assert_eq!(parse_rule("격주"), None);
        assert_eq!(parse_rule("매월 40일"), None);
        assert_eq!(parse_rule("매년 13-1"), None);
    }

    #[test]
    fn 다음_날짜_기본() {
        let today = d("2026-07-03");
        assert_eq!(next_date(&RecurrenceRule::Daily, d("2026-07-03"), today), d("2026-07-04"));
        assert_eq!(next_date(&RecurrenceRule::Weekly(None), d("2026-07-03"), today), d("2026-07-10"));
        // 2026-07-03은 금요일 → 다음 월요일은 07-06
        assert_eq!(next_date(&RecurrenceRule::Weekly(Some(chrono::Weekday::Mon)), d("2026-07-03"), today), d("2026-07-06"));
        assert_eq!(next_date(&RecurrenceRule::Monthly(Some(15)), d("2026-07-15"), today), d("2026-08-15"));
        assert_eq!(next_date(&RecurrenceRule::Yearly(Some((3, 14))), d("2026-03-14"), today), d("2027-03-14"));
    }

    #[test]
    fn 말일_클램프() {
        let today = d("2026-01-31");
        // 1/31 → 2월엔 31일이 없음 → 2/28
        assert_eq!(next_date(&RecurrenceRule::Monthly(None), d("2026-01-31"), today), d("2026-02-28"));
    }

    #[test]
    fn 밀린_반복은_오늘_이후까지_전진() {
        // 마감일이 한참 과거여도 다음 회차는 오늘 초과
        let today = d("2026-07-03");
        assert_eq!(next_date(&RecurrenceRule::Daily, d("2026-06-01"), today), d("2026-07-04"));
        assert_eq!(next_date(&RecurrenceRule::Weekly(None), d("2026-06-05"), today), d("2026-07-10"));
    }
}
