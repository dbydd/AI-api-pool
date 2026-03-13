use crate::config::Config;
use crate::health_check::{ProviderStatus, SharedHealthChecker};
use crate::load_balancer::SharedLoadBalancer;
use crate::providers::ChatRequest;
use anyhow::Result;
use axum::{
    body::Body,
    extract::{Path, State},
    http::{Request, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use axum::routing::post;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

#[derive(Clone)]
pub struct AppState {
    pub load_balancers: Arc<RwLock<HashMap<String, SharedLoadBalancer>>>,
    pub health_checker: SharedHealthChecker,
}

impl AppState {
    pub fn new(load_balancers: HashMap<String, SharedLoadBalancer>, health_checker: SharedHealthChecker) -> Self {
        Self {
            load_balancers: Arc::new(RwLock::new(load_balancers)),
            health_checker,
        }
    }
}

async fn chat(
    State(state): State<AppState>,
    Path(model_name): Path<String>,
    Json(request): Json<ChatRequest>,
) -> Response {
    let lb = {
        let load_balancers = state.load_balancers.read();
        load_balancers.get(&model_name).cloned()
    };
    
    match lb {
        Some(lb) => {
            let mut guard = lb.write();
            match guard.forward_request(request).await {
                Ok(response) => Json(response).into_response(),
                Err(e) => AppError::UpstreamError(e.to_string()).into_response(),
            }
        }
        None => AppError::ModelNotFound(model_name).into_response(),
    }
}

async fn v1_chat(
    State(state): State<AppState>,
    Json(request): Json<ChatRequest>,
) -> Response {
    let model = request.model.clone();
    
    let lb = {
        let load_balancers = state.load_balancers.read();
        load_balancers.get(&model).cloned()
    };
    
    match lb {
        Some(lb) => {
            let mut guard = lb.write();
            match guard.forward_request(request).await {
                Ok(response) => Json(response).into_response(),
                Err(e) => AppError::UpstreamError(e.to_string()).into_response(),
            }
        }
        None => AppError::ModelNotFound(model).into_response(),
    }
}

async fn list_models(
    State(state): State<AppState>,
) -> Json<HashMap<String, Vec<ProviderStatus>>> {
    let statuses = state.health_checker.get_status();
    let mut result: HashMap<String, Vec<ProviderStatus>> = HashMap::new();
    
    for status in statuses {
        let model = "default".to_string();
        result.entry(model).or_insert_with(Vec::new).push(status);
    }
    
    Json(result)
}

async fn health() -> &'static str {
    "OK"
}

async fn proxy_request(
    State(state): State<AppState>,
    Path((model_name, tail)): Path<(String, String)>,
    request: Request<Body>,
) -> Response {
    let result: Result<Response, AppError> = async move {
        let (upstream_url, api_key) = {
            let load_balancers = state.load_balancers.read();
            let lb = load_balancers.get(&model_name)
                .ok_or_else(|| AppError::ModelNotFound(model_name.clone()))?;
            
            let mut guard = lb.write();
            let provider = guard.get_available_provider()
                .ok_or_else(|| AppError::NoAvailableProvider)?;
            
            let p = provider.read();
            let url = format!("{}/{}", p.config.api_base, tail);
            let key = p.config.api_key.clone();
            (url, key)
        };

        let method_str = request.method().as_str();
        let method = reqwest::Method::from_bytes(method_str.as_bytes())
            .map_err(|_| AppError::UpstreamError("Invalid method".to_string()))?;
        let headers = request.headers().clone();
        let body = axum::body::to_bytes(request.into_body(), usize::MAX).await
            .map_err(|e| AppError::UpstreamError(e.to_string()))?;

        let client = reqwest::Client::new();
        let mut req_builder = client.request(method, &upstream_url)
            .header("Authorization", format!("Bearer {}", api_key));
        
        for (key, value) in headers.iter() {
            if key != "host" && key != "authorization" {
                req_builder = req_builder.header(key.as_str(), value.to_str().unwrap_or(""));
            }
        }

        let response = req_builder
            .body(body)
            .send()
            .await
            .map_err(|e| AppError::UpstreamError(e.to_string()))?;

        let status_code = response.status().as_u16();
        let status = StatusCode::from_u16(status_code)
            .map_err(|e| AppError::UpstreamError(e.to_string()))?;
        let headers = response.headers().clone();
        let body = response.bytes().await
            .map_err(|e| AppError::UpstreamError(e.to_string()))?;

        let mut builder = Response::builder().status(status);
        for (key, value) in headers.iter() {
            builder = builder.header(key.as_str(), value.to_str().unwrap_or(""));
        }

        Ok(builder.body(Body::from(body)).unwrap())
    }.await;

    result.into_response()
}

pub enum AppError {
    ModelNotFound(String),
    NoAvailableProvider,
    UpstreamError(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match self {
            AppError::ModelNotFound(name) => {
                (StatusCode::NOT_FOUND, format!("Model not found: {}", name)).into_response()
            }
            AppError::NoAvailableProvider => {
                (StatusCode::SERVICE_UNAVAILABLE, "No available provider").into_response()
            }
            AppError::UpstreamError(msg) => {
                (StatusCode::BAD_GATEWAY, format!("Upstream error: {}", msg)).into_response()
            }
        }
    }
}

pub async fn start_server(config: Config) -> Result<()> {
    let cors = CorsLayer::new()
        .allow_methods(Any)
        .allow_origin(Any)
        .allow_headers(Any);

    let mut load_balancers: HashMap<String, SharedLoadBalancer> = HashMap::new();
    let mut all_providers: Vec<Arc<parking_lot::RwLock<crate::providers::Provider>>> = Vec::new();

    for (model_name, model_config) in config.models {
        let providers: Vec<Arc<parking_lot::RwLock<crate::providers::Provider>>> = model_config
            .providers
            .into_iter()
            .map(crate::providers::create_provider)
            .collect();
        
        all_providers.extend(providers.clone());
        
        let lb = crate::load_balancer::create_load_balancer(providers);
        load_balancers.insert(model_name, lb);
    }

    let health_checker = crate::health_check::create_health_checker(all_providers, 30);
    let app_state = AppState::new(load_balancers, health_checker.clone());

    let v1_chat_handler = move |State(state): State<AppState>, Json(request): Json<ChatRequest>| async move {
        let model = request.model.clone();
        
        let (api_base, api_key) = {
            let load_balancers = state.load_balancers.read();
            let lb = match load_balancers.get(&model) {
                Some(lb) => lb,
                None => return AppError::ModelNotFound(model).into_response(),
            };
            let mut lb_guard = lb.write();
            let provider = match lb_guard.get_available_provider() {
                Some(p) => p,
                None => return AppError::NoAvailableProvider.into_response(),
            };
            let p_guard = provider.read();
            (p_guard.config.api_base.clone(), p_guard.config.api_key.clone())
        };
        
        let client = reqwest::Client::new();
        let url = format!("{}/chat/completions", api_base);
        let response = client.post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await;
        
        match response {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<crate::providers::ChatResponse>().await {
                    Ok(data) => Json(data).into_response(),
                    Err(e) => AppError::UpstreamError(format!("Failed to parse response: {}", e)).into_response(),
                }
            }
            Ok(resp) => {
                let status = resp.status();
                AppError::UpstreamError(format!("Upstream returned error: {}", status)).into_response()
            }
            Err(e) => AppError::UpstreamError(format!("Request failed: {}", e)).into_response(),
        }
    };

    let chat_handler = move |State(state): State<AppState>, Path(model_name): Path<String>, Json(request): Json<ChatRequest>| async move {
        let (api_base, api_key) = {
            let load_balancers = state.load_balancers.read();
            let lb = match load_balancers.get(&model_name) {
                Some(lb) => lb,
                None => return AppError::ModelNotFound(model_name).into_response(),
            };
            let mut lb_guard = lb.write();
            let provider = match lb_guard.get_available_provider() {
                Some(p) => p,
                None => return AppError::NoAvailableProvider.into_response(),
            };
            let p_guard = provider.read();
            (p_guard.config.api_base.clone(), p_guard.config.api_key.clone())
        };
        
        let client = reqwest::Client::new();
        let url = format!("{}/chat/completions", api_base);
        let response = client.post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await;
        
        match response {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<crate::providers::ChatResponse>().await {
                    Ok(data) => Json(data).into_response(),
                    Err(e) => AppError::UpstreamError(format!("Failed to parse response: {}", e)).into_response(),
                }
            }
            Ok(resp) => {
                let status = resp.status();
                AppError::UpstreamError(format!("Upstream returned error: {}", status)).into_response()
            }
            Err(e) => AppError::UpstreamError(format!("Request failed: {}", e)).into_response(),
        }
    };

    let proxy_handler = move |State(state): State<AppState>, Path((model_name, tail)): Path<(String, String)>, request: Request<Body>| async move {
        let result: Result<Response, AppError> = async move {
            let (upstream_url, api_key) = {
                let load_balancers = state.load_balancers.read();
                let lb = load_balancers.get(&model_name)
                    .ok_or_else(|| AppError::ModelNotFound(model_name.clone()))?;
                
                let mut guard = lb.write();
                let provider = guard.get_available_provider()
                    .ok_or_else(|| AppError::NoAvailableProvider)?;
                
                let p = provider.read();
                let url = format!("{}/{}", p.config.api_base, tail);
                let key = p.config.api_key.clone();
                (url, key)
            };

            let method_str = request.method().as_str();
            let method = reqwest::Method::from_bytes(method_str.as_bytes())
                .map_err(|_| AppError::UpstreamError("Invalid method".to_string()))?;
            let headers = request.headers().clone();
            let body = axum::body::to_bytes(request.into_body(), usize::MAX).await
                .map_err(|e| AppError::UpstreamError(e.to_string()))?;

            let client = reqwest::Client::new();
            let mut req_builder = client.request(method, &upstream_url)
                .header("Authorization", format!("Bearer {}", api_key));
            
            for (key, value) in headers.iter() {
                if key != "host" && key != "authorization" {
                    req_builder = req_builder.header(key.as_str(), value.to_str().unwrap_or(""));
                }
            }

            let response = req_builder
                .body(body)
                .send()
                .await
                .map_err(|e| AppError::UpstreamError(e.to_string()))?;

            let status_code = response.status().as_u16();
            let status = StatusCode::from_u16(status_code)
                .map_err(|e| AppError::UpstreamError(e.to_string()))?;
            let headers = response.headers().clone();
            let body = response.bytes().await
                .map_err(|e| AppError::UpstreamError(e.to_string()))?;

            let mut builder = Response::builder().status(status);
            for (key, value) in headers.iter() {
                builder = builder.header(key.as_str(), value.to_str().unwrap_or(""));
            }

            Ok(builder.body(Body::from(body)).unwrap())
        }.await;

        result.into_response()
    };

    let app = Router::new()
        .route("/", get(health))
        .route("/health", get(health))
        .route("/v1/models", get(list_models))
        .route("/v1/chat/completions", post(v1_chat_handler))
        .route("/v1/models/:model_name/chat/completions", post(chat_handler))
        .route("/v1/:model_name/*tail", post(proxy_handler))
        .layer(cors)
        .with_state(app_state);

    let addr = format!("{}:{}", config.server.host, config.server.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    
    tracing::info!("Server listening on {}", addr);

    tokio::spawn(async move {
        health_checker.start_background_checking().await;
    });

    axum::serve(listener, app).await?;

    Ok(())
}
