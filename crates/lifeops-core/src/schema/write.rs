use crate::schema::raw::RawSchema;

/// RawSchema를 YAML 문자열로 직렬화한다. `type:` 키를 쓰고,
/// None/false 기본값 필드는 생략한다(깔끔한 손편집 병행을 위해).
pub fn to_yaml(raw: &RawSchema) -> Result<String, serde_yaml::Error> {
    serde_yaml::to_string(raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::raw::load_raw_dir;

    #[test]
    fn 직렬화는_type을_쓰고_none과_false를_생략하고_왕복한다() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("물건.yaml"),
            "type: 물건\ncategory: 컬렉션\nfields:\n  이름: { kind: text, required: true }\n  가격: { kind: money }\n  상태: { kind: enum, options: [위시, 보유] }\n",
        )
        .unwrap();
        let raw = load_raw_dir(dir.path()).unwrap();
        let (schema, _file) = &raw["물건"];

        let yaml = to_yaml(schema).unwrap();
        assert!(yaml.contains("type: 물건"), "type 키가 있어야 함:\n{yaml}");
        assert!(!yaml.contains("extends"), "None인 extends는 생략:\n{yaml}");
        assert!(
            !yaml.contains("field_order"),
            "None인 field_order는 생략:\n{yaml}"
        );
        let 가격_이후 = yaml.split("가격").nth(1).unwrap_or("");
        let 상태_이전 = 가격_이후.split("상태").next().unwrap_or("");
        assert!(
            !상태_이전.contains("required"),
            "가격은 required 생략:\n{yaml}"
        );

        std::fs::write(dir.path().join("물건.yaml"), &yaml).unwrap();
        let reparsed = load_raw_dir(dir.path()).unwrap();
        let (s2, _) = &reparsed["물건"];
        let names: Vec<&str> = s2.fields.keys().map(|s| s.as_str()).collect();
        assert_eq!(names, vec!["이름", "가격", "상태"]);
        assert!(s2.fields["이름"].required);
        assert_eq!(s2.category.as_deref(), Some("컬렉션"));
        assert_eq!(
            s2.fields["상태"].options.as_deref(),
            Some(&["위시".to_string(), "보유".to_string()][..])
        );
    }

    #[test]
    fn 빈_behaviors는_직렬화에서_생략한다() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("할일.yaml"),
            "type: 할일\nbehaviors: {}\nfields:\n  제목: { kind: text }\n",
        )
        .unwrap();
        let raw = load_raw_dir(dir.path()).unwrap();
        let (schema, _file) = &raw["할일"];

        let yaml = to_yaml(schema).unwrap();
        assert!(
            !yaml.contains("behaviors"),
            "빈 behaviors는 생략해야 함:\n{yaml}"
        );
        assert!(
            !yaml.contains("recurrence: null"),
            "빈 recurrence null은 쓰면 안 됨:\n{yaml}"
        );

        std::fs::write(dir.path().join("할일.yaml"), &yaml).unwrap();
        let reparsed = load_raw_dir(dir.path()).unwrap();
        let (s2, _) = &reparsed["할일"];
        assert!(s2.behaviors.is_none(), "빈 behaviors는 왕복 후 None이어야 함");
    }

    #[test]
    fn singleton_true는_직렬화되고_false는_생략되며_왕복한다() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("프로필.yaml"),
            "type: 프로필\nsingleton: true\nfields:\n  이름: { kind: text }\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("물건.yaml"),
            "type: 물건\nfields:\n  이름: { kind: text }\n",
        )
        .unwrap();
        let raw = load_raw_dir(dir.path()).unwrap();

        let y_profile = to_yaml(&raw["프로필"].0).unwrap();
        assert!(
            y_profile.contains("singleton: true"),
            "true는 써야 함:\n{y_profile}"
        );
        let y_thing = to_yaml(&raw["물건"].0).unwrap();
        assert!(!y_thing.contains("singleton"), "false는 생략:\n{y_thing}");

        std::fs::write(dir.path().join("프로필.yaml"), &y_profile).unwrap();
        let reparsed = load_raw_dir(dir.path()).unwrap();
        assert!(reparsed["프로필"].0.singleton);
    }

    #[test]
    fn recurrence_behaviors는_직렬화하고_왕복한다() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("할일.yaml"),
            "type: 할일\nbehaviors:\n  recurrence:\n    flag: 반복\n    rule: 반복규칙\n    date: 마감일\nfields:\n  제목: { kind: text }\n",
        )
        .unwrap();
        let raw = load_raw_dir(dir.path()).unwrap();
        let (schema, _file) = &raw["할일"];

        let yaml = to_yaml(schema).unwrap();
        assert!(yaml.contains("behaviors"), "behaviors가 있어야 함:\n{yaml}");
        assert!(yaml.contains("recurrence"), "recurrence가 있어야 함:\n{yaml}");
        assert!(
            !yaml.contains("recurrence: null"),
            "실제 recurrence를 null로 쓰면 안 됨:\n{yaml}"
        );

        std::fs::write(dir.path().join("할일.yaml"), &yaml).unwrap();
        let reparsed = load_raw_dir(dir.path()).unwrap();
        let (s2, _) = &reparsed["할일"];
        let recurrence = s2
            .behaviors
            .as_ref()
            .and_then(|behaviors| behaviors.recurrence.as_ref())
            .unwrap();
        assert_eq!(recurrence.flag, "반복");
        assert_eq!(recurrence.rule, "반복규칙");
        assert_eq!(recurrence.date, "마감일");
    }
}
