# AI API Pool

A Rust-based load balancer and proxy for AI API providers. Aggregates multiple API providers (OpenAI, Azure, DeepSeek, OpenRouter, etc.) for the same model with automatic failover and health checking.

## Features

- **Multi-Provider Load Balancing**: Route requests across multiple providers for the same model (e.g., DeepSeek-R1 from OpenRouter, Azure, or DeepSeek)
- **Automatic Failover**: Automatically skips providers that are unavailable or return quota exceeded errors
- **Health Checking**: Background health checks monitor provider availability every 30 seconds
- **OpenAI-Compatible API**: Drop-in replacement for OpenAI API clients
- **YAML Configuration**: Simple configuration file format
- **Model Isolation**: Each model is independently configured

## Installation

```bash
# Build the project
cargo build --release

# Run with default config
cargo run

# Run with custom config
cargo run -- config.yaml
```

## Configuration

Create a `config.yaml` file:

```yaml
server:
  host: "0.0.0.0"
  port: 8080
  config_file: "config.yaml"

models:
  deepseek-r1:
    model_name: "deepseek-r1"
    providers:
      - name: "openrouter"
        api_base: "https://openrouter.ai/api/v1"
        api_key: "${OPENROUTER_API_KEY}"
        enabled: true
      - name: "azure"
        api_base: "https://your-resource.openai.azure.com"
        api_key: "${AZURE_API_KEY}"
        enabled: true
      - name: "deepseek"
        api_base: "https://api.deepseek.com/v1"
        api_key: "${DEEPSEEK_API_KEY}"
        enabled: true

  gpt-4o:
    model_name: "gpt-4o"
    providers:
      - name: "openai"
        api_base: "https://api.openai.com/v1"
        api_key: "${OPENAI_API_KEY}"
        enabled: true
      - name: "azure"
        api_base: "https://your-resource.openai.azure.com"
        api_key: "${AZURE_API_KEY}"
        enabled: true
```

### Environment Variables

API keys support environment variable substitution:

```yaml
api_key: "${OPENROUTER_API_KEY}"
```

Set them in your shell:

```bash
export OPENROUTER_API_KEY="your-key-here"
export AZURE_API_KEY="your-key-here"
export DEEPSEEK_API_KEY="your-key-here"
export OPENAI_API_KEY="your-key-here"
```

## API Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/` | GET | Health check |
| `/health` | GET | Health check |
| `/v1/models` | GET | List available models and providers |
| `/v1/chat/completions` | POST | OpenAI-compatible chat endpoint |
| `/v1/models/:model_name/chat/completions` | POST | Chat completions for specific model |
| `/v1/:model_name/*tail` | POST | Generic proxy for any endpoint |

### Usage Examples

#### Using OpenAI SDK

```python
from openai import OpenAI

client = OpenAI(
    base_url="http://localhost:8080/v1",
    api_key="dummy"  # Required but unused
)

response = client.chat.completions.create(
    model="gpt-4o",
    messages=[{"role": "user", "content": "Hello!"}]
)
```

#### Using cURL

```bash
curl -X POST http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "deepseek-r1",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

## Architecture

```
┌─────────────┐
│   Client    │
└──────┬──────┘
       │
       ▼
┌─────────────────────────────────────────┐
│            AI API Pool Server           │
│  (Axum + Tokio)                        │
│                                         │
│  ┌─────────────┐  ┌─────────────────┐  │
│  │  Router     │  │ Health Checker   │  │
│  │  (Routes)   │  │ (Background)     │  │
│  └──────┬──────┘  └────────┬────────┘  │
│         │                 │           │
│         ▼                 │           │
│  ┌─────────────┐          │           │
│  │LoadBalancer │◄─────────┘           │
│  │ (Round-Robin)                       │
│  └──────┬──────┘                        │
│         │                               │
└─────────┼───────────────────────────────┘
          │
    ┌─────┴─────┬────────────┐
    ▼           ▼            ▼
┌───────┐  ┌───────┐   ┌───────┐
│Provider│ │Provider│   │Provider│
│(OpenAI)│ │(Azure) │   │(DeepSeek)│
└───────┘  └───────┘   └───────┘
```

## Project Structure

```
src/
├── main.rs         # Entry point
├── lib.rs          # Library exports
├── config.rs       # Configuration loading
├── server.rs       # HTTP server and routes
├── load_balancer.rs # Provider selection logic
├── providers/      # Provider implementations
│   └── mod.rs
└── health_check.rs # Provider health monitoring
```

## Tech Stack

- **Runtime**: Tokio (async)
- **Web Framework**: Axum
- **HTTP Client**: Reqwest
- **Serialization**: Serde (YAML/JSON)
- **Logging**: Tracing

## License

MIT
