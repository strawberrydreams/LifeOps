pub mod convert;
pub mod logic;
pub mod params;

use crate::state::AppState;

/// MCP 서버 인스턴스. 세션마다 새로 만들어지며 Arc 기반 AppState를 공유한다.
#[derive(Clone)]
pub struct LifeOpsMcp {
    pub(crate) state: AppState,
}

impl LifeOpsMcp {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}
