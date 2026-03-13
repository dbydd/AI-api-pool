use crate::providers::Provider;
use parking_lot::RwLock;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;
use reqwest::Client;

pub struct HealthChecker {
    providers: Vec<Arc<RwLock<Provider>>>,
    check_interval: Duration,
}

impl HealthChecker {
    pub fn new(providers: Vec<Arc<RwLock<Provider>>>, check_interval_secs: u64) -> Self {
        Self {
            providers,
            check_interval: Duration::from_secs(check_interval_secs),
        }
    }

    pub async fn check_all(&self) {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| Client::new());

        for provider in &self.providers {
            // Extract config synchronously while holding the lock
            let (url, api_key) = {
                let guard = provider.read();
                guard.health_check_info()
            };
            // Now do the async health check OUTSIDE the lock
            let is_healthy = perform_health_check(&client, &url, &api_key).await;
            // Re-acquire lock to update status
            provider.write().set_available(is_healthy);
        }
    }

    pub async fn start_background_checking(&self) {
        let mut ticker = interval(self.check_interval);
        
        loop {
            ticker.tick().await;
            self.check_all().await;
        }
    }

    pub fn get_status(&self) -> Vec<ProviderStatus> {
        self.providers.iter().map(|p| {
            let guard = p.read();
            ProviderStatus {
                name: guard.name().to_string(),
                available: guard.is_available(),
                api_base: guard.config.api_base.clone(),
            }
        }).collect()
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProviderStatus {
    pub name: String,
    pub available: bool,
    pub api_base: String,
}

pub type SharedHealthChecker = Arc<HealthChecker>;

pub fn create_health_checker(providers: Vec<Arc<RwLock<Provider>>>, interval_secs: u64) -> SharedHealthChecker {
    Arc::new(HealthChecker::new(providers, interval_secs))
}

/// Perform health check - takes ownership of client, url, key so no locks needed
async fn perform_health_check(client: &Client, url: &str, api_key: &str) -> bool {
    match client
        .get(url)
        .header("Authorization", format!("Bearer {}", api_key))
        .send()
        .await
    {
        Ok(response) => response.status().is_success(),
        Err(_) => false,
    }
}
