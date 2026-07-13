use crate::state::AppState;
use axum::extract::State;
use axum::Json;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr};

/// GET /api/system/info → 데이터 경로·포트·LAN 접속 주소.
pub async fn info(State(state): State<AppState>) -> Json<Value> {
    let port = state.bound_addr.port();
    Json(json!({
        "data_dir": state.data_dir.display().to_string(),
        "port": port,
        "lan_addrs": lan_addresses(port),
    }))
}

/// 현재 호스트의 사설 IPv4 인터페이스를 중복 없는 결정적 URL 목록으로 만든다.
pub fn lan_addresses(port: u16) -> Vec<String> {
    let interfaces = local_ip_address::list_afinet_netifas().unwrap_or_default();
    lan_addresses_from(port, interfaces.into_iter().map(|(_, ip)| ip))
}

fn lan_addresses_from(port: u16, addresses: impl IntoIterator<Item = IpAddr>) -> Vec<String> {
    addresses
        .into_iter()
        .filter_map(|ip| match ip {
            IpAddr::V4(ip) if is_lan_ipv4(ip) => Some(ip),
            _ => None,
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|ip| format!("http://{ip}:{port}"))
        .collect()
}

fn is_lan_ipv4(ip: Ipv4Addr) -> bool {
    ip.is_private() && !ip.is_loopback()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::build_app;
    use crate::state::test_state;
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    use tower::ServiceExt;

    #[tokio::test]
    async fn system_info_형태와_state_값() {
        let (state, _dir) = test_state().await;
        let expected_data_dir = state.data_dir.display().to_string();
        let expected_port = state.bound_addr.port();
        let app = build_app(state);
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/system/info")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        let body = to_bytes(res.into_body(), 1 << 20).await.unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["data_dir"], expected_data_dir);
        assert_eq!(value["port"], expected_port);
        assert!(value["lan_addrs"].is_array());
    }

    #[tokio::test]
    async fn system_info_추가가_기존_health_route를_가리지_않는다() {
        let (state, _dir) = test_state().await;
        let res = build_app(state)
            .oneshot(
                Request::builder()
                    .uri("/api/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        let body = to_bytes(res.into_body(), 1 << 20).await.unwrap();
        assert_eq!(&body[..], b"ok");
    }

    #[test]
    fn lan_주소는_사설_ipv4만_중복없이_결정적_순서로_만든다() {
        let interfaces = [
            ("z", IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20))),
            ("loopback", IpAddr::V4(Ipv4Addr::LOCALHOST)),
            ("a", IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2))),
            ("duplicate", IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20))),
            ("public", IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))),
            ("ipv6", IpAddr::V6(Ipv6Addr::LOCALHOST)),
        ];

        assert_eq!(
            lan_addresses_from(3000, interfaces.into_iter().map(|(_, ip)| ip)),
            vec!["http://10.0.0.2:3000", "http://192.168.1.20:3000",]
        );
    }
}
