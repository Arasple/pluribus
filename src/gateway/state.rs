//! Gateway 应用状态

use std::sync::Arc;

use crate::providers::Provider;

/// Gateway 应用状态
#[derive(Clone)]
pub struct AppState {
    providers: Arc<Vec<Arc<dyn Provider>>>,
}

const UTILIZATION_THRESHOLD: f64 = 0.995;

fn is_provider_available(provider: &Arc<dyn crate::providers::Provider>) -> bool {
    if let Some(rate_limit) = provider.rate_limit_info() {
        if rate_limit.seven_day.utilization > UTILIZATION_THRESHOLD {
            return false;
        }
        if rate_limit.five_hour.utilization > UTILIZATION_THRESHOLD {
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
