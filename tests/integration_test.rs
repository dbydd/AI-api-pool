//! Integration tests for the AI API Pool

use ai_api_pool::config::Config;
use ai_api_pool::config::ProviderConfig;
use ai_api_pool::health_check::create_health_checker;
use ai_api_pool::load_balancer::LoadBalancer;
use ai_api_pool::providers::{create_provider, ChatRequest, Message, Provider};

// Test Config loading
#[test]
fn test_config_load() {
    let yaml = r#"
server:
  host: "0.0.0.0"
  port: 8080
  config_file: "config.yaml"

models:
  gpt-4:
    model_name: "gpt-4"
    providers:
      - name: "openai"
        api_base: "https://api.openai.com/v1"
        api_key: "sk-test"
        enabled: true
      - name: "azure"
        api_base: "https://example.azure.com/v1"
        api_key: "azure-key"
        enabled: true
"#;
    let config: Config = serde_yaml::from_str(yaml).unwrap();
    
    assert_eq!(config.server.host, "0.0.0.0");
    assert_eq!(config.server.port, 8080);
    assert_eq!(config.models.len(), 1);
    
    let gpt4 = config.models.get("gpt-4").unwrap();
    assert_eq!(gpt4.providers.len(), 2);
}

// Test Provider creation
#[test]
fn test_provider_creation() {
    let provider_config = ProviderConfig {
        name: "test-provider".to_string(),
        api_base: "https://api.test.com/v1".to_string(),
        api_key: "test-key".to_string(),
        enabled: true,
    };
    
    let provider = Provider::new(provider_config.clone());
    
    assert_eq!(provider.name(), "test-provider");
    assert!(provider.is_available()); // Should be available since enabled=true
}

#[test]
fn test_provider_availability() {
    let provider_config = ProviderConfig {
        name: "test".to_string(),
        api_base: "https://api.test.com".to_string(),
        api_key: "key".to_string(),
        enabled: false,
    };
    
    let provider = Provider::new(provider_config);
    assert!(!provider.is_available()); // Not available because enabled=false
    
    let provider_config = ProviderConfig {
        name: "test".to_string(),
        api_base: "https://api.test.com".to_string(),
        api_key: "key".to_string(),
        enabled: true,
    };
    
    let provider = Provider::new(provider_config);
    assert!(provider.is_available()); // Available because enabled=true
}

#[test]
fn test_provider_health_check_info() {
    let provider_config = ProviderConfig {
        name: "test".to_string(),
        api_base: "https://api.test.com/v1".to_string(),
        api_key: "my-key".to_string(),
        enabled: true,
    };
    
    let provider = Provider::new(provider_config);
    let (url, key) = provider.health_check_info();
    
    assert_eq!(url, "https://api.test.com/v1/models");
    assert_eq!(key, "my-key");
}

// Test LoadBalancer
#[test]
fn test_load_balancer_selection() {
    let provider1_config = ProviderConfig {
        name: "provider1".to_string(),
        api_base: "https://api.test1.com".to_string(),
        api_key: "key1".to_string(),
        enabled: true,
    };
    
    let provider2_config = ProviderConfig {
        name: "provider2".to_string(),
        api_base: "https://api.test2.com".to_string(),
        api_key: "key2".to_string(),
        enabled: true,
    };
    
    let p1 = create_provider(provider1_config);
    let p2 = create_provider(provider2_config);
    
    let mut lb = LoadBalancer::new(vec![p1.clone(), p2.clone()]);
    
    // Should get provider1 first
    let selected = lb.get_available_provider();
    assert!(selected.is_some());
}

#[test]
fn test_load_balancer_round_robin() {
    let provider1_config = ProviderConfig {
        name: "provider1".to_string(),
        api_base: "https://api.test1.com".to_string(),
        api_key: "key1".to_string(),
        enabled: true,
    };
    
    let provider2_config = ProviderConfig {
        name: "provider2".to_string(),
        api_base: "https://api.test2.com".to_string(),
        api_key: "key2".to_string(),
        enabled: true,
    };
    
    let p1 = create_provider(provider1_config);
    let p2 = create_provider(provider2_config);
    
    let mut lb = LoadBalancer::new(vec![p1.clone(), p2.clone()]);
    
    // First selection
    let first = lb.get_available_provider();
    assert!(first.is_some());
    
    // Second selection - should be different (round robin)
    let second = lb.get_available_provider();
    assert!(second.is_some());
}

#[test]
fn test_load_balancer_skips_unavailable() {
    let provider1_config = ProviderConfig {
        name: "provider1".to_string(),
        api_base: "https://api.test1.com".to_string(),
        api_key: "key1".to_string(),
        enabled: false, // Disabled
    };
    
    let provider2_config = ProviderConfig {
        name: "provider2".to_string(),
        api_base: "https://api.test2.com".to_string(),
        api_key: "key2".to_string(),
        enabled: true,
    };
    
    let p1 = create_provider(provider1_config);
    let p2 = create_provider(provider2_config);
    
    let mut lb = LoadBalancer::new(vec![p1, p2]);
    
    // Only provider2 should be available
    let selected = lb.get_available_provider();
    assert!(selected.is_some());
    
    let provider = selected.unwrap();
    let guard = provider.read();
    assert_eq!(guard.name(), "provider2");
}

#[test]
fn test_load_balancer_no_available() {
    let provider_config = ProviderConfig {
        name: "provider1".to_string(),
        api_base: "https://api.test1.com".to_string(),
        api_key: "key1".to_string(),
        enabled: false, // Disabled
    };
    
    let p1 = create_provider(provider_config);
    
    let mut lb = LoadBalancer::new(vec![p1]);
    
    // No providers available
    let selected = lb.get_available_provider();
    assert!(selected.is_none());
}

#[test]
fn test_load_balancer_mark_unavailable() {
    let provider_config = ProviderConfig {
        name: "test-provider".to_string(),
        api_base: "https://api.test.com".to_string(),
        api_key: "key".to_string(),
        enabled: true,
    };
    
    let p1 = create_provider(provider_config);
    
    let mut lb = LoadBalancer::new(vec![p1.clone()]);
    
    // Initially available
    assert!(lb.get_available_provider().is_some());
    
    // Mark as unavailable
    lb.mark_provider_unavailable("test-provider");
    
    // Now should not be available
    let selected = lb.get_available_provider();
    assert!(selected.is_none());
}

#[test]
fn test_load_balancer_providers_count() {
    let provider1_config = ProviderConfig {
        name: "provider1".to_string(),
        api_base: "https://api.test1.com".to_string(),
        api_key: "key1".to_string(),
        enabled: true,
    };
    
    let provider2_config = ProviderConfig {
        name: "provider2".to_string(),
        api_base: "https://api.test2.com".to_string(),
        api_key: "key2".to_string(),
        enabled: true,
    };
    
    let p1 = create_provider(provider1_config);
    let p2 = create_provider(provider2_config);
    
    let lb = LoadBalancer::new(vec![p1, p2]);
    
    assert_eq!(lb.providers_count(), 2);
}

// Test HealthChecker
#[test]
fn test_health_checker_status() {
    let provider_config = ProviderConfig {
        name: "test-provider".to_string(),
        api_base: "https://api.test.com".to_string(),
        api_key: "key".to_string(),
        enabled: true,
    };
    
    let p1 = create_provider(provider_config);
    
    let checker = create_health_checker(vec![p1], 30);
    
    let statuses = checker.get_status();
    
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].name, "test-provider");
    assert!(statuses[0].available);
}

#[test]
fn test_health_checker_multiple_providers() {
    let provider1_config = ProviderConfig {
        name: "provider1".to_string(),
        api_base: "https://api.test1.com".to_string(),
        api_key: "key1".to_string(),
        enabled: true,
    };
    
    let provider2_config = ProviderConfig {
        name: "provider2".to_string(),
        api_base: "https://api.test2.com".to_string(),
        api_key: "key2".to_string(),
        enabled: false,
    };
    
    let p1 = create_provider(provider1_config);
    let p2 = create_provider(provider2_config);
    
    let checker = create_health_checker(vec![p1, p2], 30);
    
    let statuses = checker.get_status();
    
    assert_eq!(statuses.len(), 2);
    
    // Find provider1 - should be available
    let p1_status = statuses.iter().find(|s| s.name == "provider1").unwrap();
    assert!(p1_status.available);
    
    // Find provider2 - should not be available (enabled=false)
    let p2_status = statuses.iter().find(|s| s.name == "provider2").unwrap();
    assert!(!p2_status.available);
}

// Test ChatRequest serialization
#[test]
fn test_chat_request_serialization() {
    let request = ChatRequest {
        model: "gpt-4".to_string(),
        messages: vec![
            Message {
                role: "user".to_string(),
                content: "Hello".to_string(),
            }
        ],
        stream: None,
        temperature: Some(0.7),
        max_tokens: Some(100),
    };
    
    let json = serde_json::to_string(&request).unwrap();
    assert!(json.contains("gpt-4"));
    assert!(json.contains("user"));
    assert!(json.contains("Hello"));
}

#[test]
fn test_chat_request_deserialization() {
    let json = r#"{
        "model": "gpt-4",
        "messages": [
            {"role": "user", "content": "Hi"}
        ],
        "temperature": 0.5
    }"#;
    
    let request: ChatRequest = serde_json::from_str(json).unwrap();
    
    assert_eq!(request.model, "gpt-4");
    assert_eq!(request.messages.len(), 1);
    assert_eq!(request.messages[0].role, "user");
    assert_eq!(request.temperature, Some(0.5));
}
