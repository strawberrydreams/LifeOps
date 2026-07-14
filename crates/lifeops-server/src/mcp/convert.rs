use lifeops_core::error::{CoreError, ViewError};

/// 코어 에러를 AI가 읽고 자가 수정할 수 있는 한국어 문장으로 변환한다.
pub fn core_error_text(error: &CoreError) -> String {
    match error {
        CoreError::Validation(validation) => {
            let fields = validation
                .0
                .iter()
                .map(|field| format!("필드 '{}': {}", field.field, field.message))
                .collect::<Vec<_>>();
            format!("검증 실패 — {}", fields.join(" / "))
        }
        CoreError::UnknownType(entity_type) => format!(
            "알 수 없는 타입 '{entity_type}'. list_types로 존재하는 타입을 먼저 확인하세요."
        ),
        CoreError::NotFound(id) => format!("엔티티를 찾을 수 없음: {id}"),
        CoreError::DeleteBlocked { referrers } => {
            let references = referrers
                .iter()
                .map(|reference| {
                    format!(
                        "{}({}).{}",
                        reference.from_type, reference.from_id, reference.field_name
                    )
                })
                .collect::<Vec<_>>();
            format!(
                "삭제 불가 — {}곳에서 참조 중: {}. 참조를 먼저 정리하세요.",
                references.len(),
                references.join(", ")
            )
        }
        CoreError::SingletonExists(entity_type) => format!(
            "싱글턴 타입 '{entity_type}'은 하나만 존재할 수 있습니다. 기존 것을 update_entity로 수정하세요."
        ),
        CoreError::Db(_) => "내부 저장소 오류가 발생했습니다.".to_string(),
    }
}

/// 뷰/쿼리 에러를 한국어 문장으로 바꾼다. 코어 에러는 같은 변환을 재사용한다.
pub fn view_error_text(error: &ViewError) -> String {
    match error {
        ViewError::Core(core) => core_error_text(core),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lifeops_core::entity::store::RefEdge;
    use lifeops_core::entity::validate::{FieldError, ValidationError};

    #[test]
    fn 검증에러는_필드별_사유를_문장으로() {
        let error = CoreError::Validation(ValidationError(vec![
            FieldError {
                field: "이름".into(),
                message: "필수 필드".into(),
            },
            FieldError {
                field: "가격".into(),
                message: "money 객체 {amount, currency} 필요".into(),
            },
        ]));
        let text = core_error_text(&error);
        assert!(text.contains("이름"));
        assert!(text.contains("필수 필드"));
        assert!(text.contains("가격"));
        assert!(text.contains("money 객체"));
    }

    #[test]
    fn 삭제차단은_참조목록을_문장으로() {
        let error = CoreError::DeleteBlocked {
            referrers: vec![RefEdge {
                from_id: "t1".into(),
                from_type: "할일".into(),
                field_name: "관련물건".into(),
            }],
        };
        let text = core_error_text(&error);
        assert!(text.contains("참조"));
        assert!(text.contains("할일"));
        assert!(text.contains("t1"));
        assert!(text.contains("관련물건"));
    }

    #[test]
    fn 없는타입과_없는id는_명확한_문장() {
        assert!(core_error_text(&CoreError::UnknownType("유령".into())).contains("유령"));
        assert!(core_error_text(&CoreError::NotFound("x".into())).contains("x"));
        assert!(core_error_text(&CoreError::SingletonExists("프로필".into())).contains("프로필"));
    }
}
