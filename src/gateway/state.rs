//! Gateway 应用状态

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crate::providers::Provider;

/// Gateway 应用状态
#[derive(Clone)]
pub struct AppState {
    providers: Arc<Vec<Arc<dyn Provider>>>,
    counter: Arc<AtomicUsize>,
}

impl AppState {
    pub fn new(providers: Vec<Arc<dyn Provider>>) -> Self {
        Self {
            providers: Arc::new(providers),
            counter: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// 获取所有 providers
    pub fn providers(&self) -> &[Arc<dyn Provider>] {
        &self.providers
    }

    /// 选择下一个 provider
    pub fn get_next_provider<F>(&self, filter: F) -> Option<Arc<dyn Provider>>
    where
        F: FnMut(&&Arc<dyn Provider>) -> bool,
    {
        let filtered: Vec<_> = self.providers.iter().filter(filter).collect();

        if filtered.is_empty() {
            return None;
        }

        let idx = self.counter.fetch_add(1, Ordering::Relaxed) % filtered.len();
        Some(filtered[idx].clone())
    }
}
