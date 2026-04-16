mod raw;
mod resolved;

use arc_swap::ArcSwap;
use std::{ops::Deref, sync::Arc};

pub use resolved::*;

#[derive(Debug, Clone)]
pub struct ProxyConfig(Arc<ArcSwap<ProxyConfigResolved>>);

impl ProxyConfig {
    pub fn new(config: ProxyConfigResolved) -> Self {
        Self(Arc::new(ArcSwap::new(Arc::new(config))))
    }

    pub fn update(&self, config: ProxyConfigResolved) {
        self.0.store(Arc::new(config))
    }

    pub fn get_full(&self) -> Arc<ProxyConfigResolved> {
        self.0.load_full()
    }
}

impl Deref for ProxyConfig {
    type Target = Arc<ArcSwap<ProxyConfigResolved>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
