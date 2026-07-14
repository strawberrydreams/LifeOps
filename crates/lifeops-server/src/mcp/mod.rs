pub mod auth;
pub mod convert;
pub mod logic;
pub mod params;
pub mod prompt;
pub mod tools;

use crate::state::AppState;
use axum::extract::{ConnectInfo, State};
use axum::http::{header::AUTHORIZATION, HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use rmcp::handler::server::router::prompt::PromptRouter;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::{ProtocolVersion, ServerCapabilities, ServerInfo};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::ServerHandler;
use std::collections::BTreeSet;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

/// MCP 서버 인스턴스. 세션마다 새로 만들어지며 Arc 기반 AppState를 공유한다.
#[derive(Clone)]
pub struct LifeOpsMcp {
    pub(crate) state: AppState,
    tool_router: ToolRouter<Self>,
    prompt_router: PromptRouter<Self>,
}

impl LifeOpsMcp {
    pub fn new(state: AppState) -> Self {
        Self {
            state,
            tool_router: Self::tool_router(),
            prompt_router: Self::prompt_router(),
        }
    }
}

#[rmcp::tool_handler(router = self.tool_router)]
#[rmcp::prompt_handler(router = self.prompt_router)]
impl ServerHandler for LifeOpsMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_prompts()
                .build(),
        )
        .with_protocol_version(ProtocolVersion::LATEST)
        .with_instructions(
                "LifeOps는 개인 Life Context 저장소입니다. 데이터는 타입별 엔티티로 저장됩니다.\n\
                 - 무엇이든 넣기 전에 list_types / get_schema로 스키마를 먼저 확인하세요.\n\
                 - 스키마에 맞는 타입이 없으면 새로 만들지 말고 사용자에게 보고하세요(스키마·뷰 변경은 사람 전용).\n\
                 - 중복 저장을 피하려면 create_entity 전에 query_entities로 확인하고, 기존 것이면 update_entity를 쓰세요.\n\
                 - 자유 텍스트를 엔티티로 분해하려면 ingest 프롬프트를 사용하세요."
        )
    }
}

/// `/mcp`에 nest할 Streamable HTTP 서비스다.
///
/// rmcp의 DNS-rebinding 방어 기본값을 유지하면서 현재 서버가 실제로
/// 수신할 수 있는 로컬 인터페이스 IP만 Host allowlist에 추가한다.
pub fn service(state: AppState) -> StreamableHttpService<LifeOpsMcp, LocalSessionManager> {
    let allowed_hosts = allowed_hosts(
        state.bound_addr.ip(),
        local_ip_address::list_afinet_netifas()
            .unwrap_or_default()
            .into_iter()
            .map(|(_, ip)| ip),
    );
    let allowed_origins = allowed_origins(&allowed_hosts, state.bound_addr.port());
    let config = StreamableHttpServerConfig::default()
        .with_allowed_hosts(allowed_hosts)
        .with_allowed_origins(allowed_origins);

    StreamableHttpService::new(
        move || Ok(LifeOpsMcp::new(state.clone())),
        Arc::new(LocalSessionManager::default()),
        config,
    )
}

fn allowed_origins(hosts: &[String], port: u16) -> Vec<String> {
    hosts
        .iter()
        .map(|host| {
            let authority = if host.parse::<std::net::Ipv6Addr>().is_ok() {
                format!("[{host}]")
            } else {
                host.clone()
            };
            format!("http://{authority}:{port}")
        })
        .collect()
}

fn allowed_hosts(bound_ip: IpAddr, interface_ips: impl IntoIterator<Item = IpAddr>) -> Vec<String> {
    let mut hosts = BTreeSet::from([
        "localhost".to_string(),
        "127.0.0.1".to_string(),
        "::1".to_string(),
    ]);
    if !bound_ip.is_unspecified() {
        hosts.insert(bound_ip.to_string());
    }
    hosts.extend(
        interface_ips
            .into_iter()
            .filter(|ip| !ip.is_unspecified())
            .map(|ip| ip.to_string()),
    );
    hosts.into_iter().collect()
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|token| !token.is_empty())
}

/// `/mcp` 앞단 인증 미들웨어. 시작 시 확정된 정책과 실제 TCP 피어로 판정한다.
pub async fn guard(
    State(policy): State<auth::AuthPolicy>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    match policy.decide(bearer(&headers), peer.ip().is_loopback()) {
        auth::AuthDecision::Allow => next.run(request).await,
        auth::AuthDecision::Unauthorized => (
            StatusCode::UNAUTHORIZED,
            [("WWW-Authenticate", "Bearer")],
            "MCP 토큰이 필요합니다 (Authorization: Bearer <LIFEOPS_MCP_TOKEN>)",
        )
            .into_response(),
        auth::AuthDecision::Forbidden => (
            StatusCode::FORBIDDEN,
            "LAN 노출 시 MCP는 LIFEOPS_MCP_TOKEN 설정이 필요합니다",
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::test_state;

    #[tokio::test]
    async fn 도구_9종과_ingest_프롬프트가_등록된다() {
        let (state, _dir) = test_state().await;
        let mcp = LifeOpsMcp::new(state);
        let tool_names = mcp
            .tool_router
            .list_all()
            .into_iter()
            .map(|tool| tool.name.to_string())
            .collect::<Vec<_>>();

        for expected in [
            "list_types",
            "get_schema",
            "query_entities",
            "get_entity",
            "create_entity",
            "update_entity",
            "delete_entity",
            "list_pages",
            "run_page",
        ] {
            assert!(
                tool_names.contains(&expected.to_string()),
                "누락된 도구: {expected}"
            );
        }

        let prompt_names = mcp
            .prompt_router
            .list_all()
            .into_iter()
            .map(|prompt| prompt.name.to_string())
            .collect::<Vec<_>>();
        assert!(prompt_names.contains(&"ingest".to_string()));
        assert!(mcp.get_info().instructions.unwrap().contains("list_types"));
    }

    #[test]
    fn host_허용목록은_기본방어와_실제_인터페이스만_포함한다() {
        let hosts = allowed_hosts(
            "0.0.0.0".parse().unwrap(),
            [
                "192.168.10.4".parse().unwrap(),
                "10.0.0.8".parse().unwrap(),
                "0.0.0.0".parse().unwrap(),
            ],
        );

        for expected in ["localhost", "127.0.0.1", "::1", "192.168.10.4", "10.0.0.8"] {
            assert!(
                hosts.contains(&expected.to_string()),
                "누락된 Host: {expected}"
            );
        }
        assert!(!hosts.contains(&"0.0.0.0".to_string()));
        assert!(!hosts.contains(&"evil.example".to_string()));
    }

    #[test]
    fn origin_허용목록은_포트와_ipv6_괄호를_정규화한다() {
        let origins = allowed_origins(
            &["localhost".into(), "127.0.0.1".into(), "::1".into()],
            3000,
        );
        assert_eq!(
            origins,
            [
                "http://localhost:3000",
                "http://127.0.0.1:3000",
                "http://[::1]:3000",
            ]
        );
    }
}
