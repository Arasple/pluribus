//! Gateway 应用状态

use std::sync::Arc;

use crate::providers::Provider;

/// Gateway 应用状态
#[derive(Clone)]
pub struct AppState {
    providers: Arc<Vec<Arc<dyn Provider>>>,
}

const UTILIZATION_THRESHOLD: f64 = 0.995;

/// 检查单个窗口是否可用
/// 如果利用率超过阈值，但已过重置时间，仍视为可用
fn is_window_available(window: &crate::providers::RateLimitWindow) -> bool {
    if window.utilization <= UTILIZATION_THRESHOLD {
        return true;
    }
    // 利用率超过阈值，检查是否已过重置时间
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    now >= window.reset
}

fn is_provider_available(provider: &Arc<dyn crate::providers::Provider>) -> bool {
    if let Some(rate_limit) = provider.rate_limit_info() {
        if !is_window_available(&rate_limit.seven_day) {
            return false;
        }
        if !is_window_available(&rate_limit.five_hour) {
            return false;
        }
    }
    true
}

impl AppState {
    pub fn new(providers: Vec<Arc<dyn crate::providers::Provider>>) -> Self {
        Self {
            providers: Arc::new(providers),
        }
    }

    pub fn providers(&self) -> &[Arc<dyn crate::providers::Provider>] {
        &self.providers
    }

    /// 按优先级顺序选择第一个可用的 provider
    pub fn get_next_provider<F>(&self, filter: F) -> Option<Arc<dyn crate::providers::Provider>>
    where
        F: FnMut(&&Arc<dyn crate::providers::Provider>) -> bool,
    {
        self.providers
            .iter()
            .filter(|p| is_provider_available(p))
            .find(filter)
            .cloned()
    }
}
