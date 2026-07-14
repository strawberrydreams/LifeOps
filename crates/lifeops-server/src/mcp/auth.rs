const TOKEN_ENV: &str = "LIFEOPS_MCP_TOKEN";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthDecision {
    Allow,
    Unauthorized,
    Forbidden,
}

/// 프로세스 시작 시 확정한 MCP 인증 정책이다.
///
/// 요청마다 환경 변수를 다시 읽지 않아 실행 중 정책이 흔들리지 않으며,
/// 테스트에서는 프로세스 전역 환경을 변경하지 않고 정책을 주입할 수 있다.
#[derive(Clone, PartialEq, Eq)]
pub struct AuthPolicy {
    token: Option<String>,
}

impl AuthPolicy {
    pub fn from_env() -> Self {
        Self::from_token(std::env::var(TOKEN_ENV).ok())
    }

    pub(crate) fn from_token(token: Option<String>) -> Self {
        Self {
            token: token
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
        }
    }

    pub(crate) fn decide(&self, bearer: Option<&str>, peer_is_loopback: bool) -> AuthDecision {
        decide(self.token.as_deref(), bearer, peer_is_loopback)
    }
}

/// 인증 정책:
/// - 토큰 설정됨 → Bearer가 정확히 일치해야 허용, 아니면 401.
/// - 토큰 없음 → loopback 피어만 허용, 그 외는 403.
pub fn decide(
    env_token: Option<&str>,
    bearer: Option<&str>,
    peer_is_loopback: bool,
) -> AuthDecision {
    match env_token {
        Some(token) if bearer == Some(token) => AuthDecision::Allow,
        Some(_) => AuthDecision::Unauthorized,
        None if peer_is_loopback => AuthDecision::Allow,
        None => AuthDecision::Forbidden,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 토큰이_있고_일치하면_피어에_관계없이_허용한다() {
        assert_eq!(
            decide(Some("s3cr3t"), Some("s3cr3t"), false),
            AuthDecision::Allow
        );
        assert_eq!(
            decide(Some("s3cr3t"), Some("s3cr3t"), true),
            AuthDecision::Allow
        );
    }

    #[test]
    fn 토큰이_있고_불일치_또는_누락이면_401이다() {
        assert_eq!(
            decide(Some("s3cr3t"), Some("wrong"), true),
            AuthDecision::Unauthorized
        );
        assert_eq!(
            decide(Some("s3cr3t"), None, true),
            AuthDecision::Unauthorized
        );
    }

    #[test]
    fn 토큰이_없고_loopback이면_허용한다() {
        assert_eq!(decide(None, None, true), AuthDecision::Allow);
        assert_eq!(
            decide(None, Some("무시되는-토큰"), true),
            AuthDecision::Allow
        );
    }

    #[test]
    fn 토큰이_없고_비loopback이면_403이다() {
        assert_eq!(decide(None, None, false), AuthDecision::Forbidden);
        assert_eq!(
            decide(None, Some("anything"), false),
            AuthDecision::Forbidden
        );
    }

    #[test]
    fn 빈_환경변수_토큰은_미설정으로_정규화한다() {
        let policy = AuthPolicy::from_token(Some(String::new()));
        assert_eq!(policy.decide(None, true), AuthDecision::Allow);
        assert_eq!(policy.decide(None, false), AuthDecision::Forbidden);

        let whitespace = AuthPolicy::from_token(Some("   ".to_string()));
        assert_eq!(whitespace.decide(None, true), AuthDecision::Allow);
    }

    #[test]
    fn 토큰_앞뒤_공백은_제거한다() {
        let policy = AuthPolicy::from_token(Some("  s3cr3t  ".to_string()));
        assert_eq!(policy.decide(Some("s3cr3t"), false), AuthDecision::Allow);
    }
}
