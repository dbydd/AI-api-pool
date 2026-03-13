use crate::config::ProviderConfig;
use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct Provider {
    pub config: ProviderConfig,
    client: Client,
    available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub stream: Option<bool>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub id: Option<String>,
    pub object: Option<String>,
    pub created: Option<u64>,
    pub model: Option<String>,
    pub choices: Option<Vec<Choice>>,
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Choice {
    pub index: Option<u32>,
    pub message: Option<Message>,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
    pub total_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: ErrorDetail,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorDetail {
    pub message: String,
    pub r#type: Option<String>,
    pub code: Option<String>,
}

impl Provider {
    pub fn new(config: ProviderConfig) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .expect("Failed to create HTTP client");
        
        Self {
            config,
            client,
            available: true,
        }
    }

    pub fn name(&self) -> &str {
        &self.config.name
    }

    pub fn is_available(&self) -> bool {
        self.available && self.config.enabled
    }

    pub fn set_available(&mut self, available: bool) {
        self.available = available;
    }

    pub async fn chat(&self, request: ChatRequest) -> Result<ChatResponse> {
        let url = format!("{}/chat/completions", self.config.api_base);
        
        let mut req_builder = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json");

        if let Some(model) = request.model.split('/').last() {
            req_builder = req_builder.json(&ChatRequest {
                model: model.to_string(),
                messages: request.messages,
                stream: request.stream,
                temperature: request.temperature,
                max_tokens: request.max_tokens,
            });
        } else {
            req_builder = req_builder.json(&request);
        }

        let response = req_builder.send().await?;
        
        if response.status().is_success() {
            let chat_response: ChatResponse = response.json().await?;
            Ok(chat_response)
        } else {
            let error: ErrorResponse = response.json().await?;
            anyhow::bail!("API error: {}", error.error.message)
        }
    }

    pub async fn health_check(&self) -> bool {
        let url = format!("{}/models", self.config.api_base);
        
        match self.client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
        {
            Ok(response) => response.status().is_success(),
            Err(_) => false,
        }
    }

    /// Returns config needed for health check - synchronous, no await
    pub fn health_check_info(&self) -> (String, String) {
        (
            format!("{}/models", self.config.api_base),
            self.config.api_key.clone(),
        )
    }
}

pub type SharedProvider = Arc<parking_lot::RwLock<Provider>>;

pub fn create_provider(config: ProviderConfig) -> SharedProvider {
    Arc::new(parking_lot::RwLock::new(Provider::new(config)))
}
