use crate::providers::{ChatRequest, Provider};
use anyhow::Result;
use parking_lot::RwLock;
use std::sync::Arc;

pub struct LoadBalancer {
    providers: Vec<Arc<RwLock<Provider>>>,
    current_index: usize,
}

impl LoadBalancer {
    pub fn new(providers: Vec<Arc<RwLock<Provider>>>) -> Self {
        Self {
            providers,
            current_index: 0,
        }
    }

    pub fn get_available_provider(&mut self) -> Option<Arc<RwLock<Provider>>> {
        let total = self.providers.len();
        if total == 0 {
            return None;
        }

        for _ in 0..total {
            let provider = &self.providers[self.current_index];
            self.current_index = (self.current_index + 1) % total;
            
            if provider.read().is_available() {
                return Some(provider.clone());
            }
        }

        None
    }

    pub async fn forward_request(&mut self, request: ChatRequest) -> Result<crate::providers::ChatResponse> {
        let provider = self.get_available_provider()
            .ok_or_else(|| anyhow::anyhow!("No available providers"))?;

        let (chat_result, is_available) = {
            let mut provider_guard = provider.write();
            let result = provider_guard.chat(request.clone()).await;
            let is_available = match &result {
                Ok(_) => {
                    provider_guard.set_available(true);
                    true
                }
                Err(e) => {
                    let error_msg = e.to_string().to_lowercase();
                    if error_msg.contains("insufficient") || 
                       error_msg.contains("quota") || 
                       error_msg.contains("balance") ||
                       error_msg.contains("not enough") ||
                       error_msg.contains("billing") ||
                       error_msg.contains("429") {
                        provider_guard.set_available(false);
                        false
                    } else {
                        true
                    }
                }
            };
            (result, is_available)
        };
        
        if !is_available {
            anyhow::bail!("Provider quota exceeded");
        }
        
        chat_result
    }

    pub fn mark_provider_unavailable(&mut self, provider_name: &str) {
        for provider in &self.providers {
            let mut guard = provider.write();
            if guard.name() == provider_name {
                guard.set_available(false);
            }
        }
    }

    pub fn providers_count(&self) -> usize {
        self.providers.len()
    }
}

pub type SharedLoadBalancer = Arc<RwLock<LoadBalancer>>;

pub fn create_load_balancer(providers: Vec<Arc<RwLock<Provider>>>) -> SharedLoadBalancer {
    Arc::new(RwLock::new(LoadBalancer::new(providers)))
}
