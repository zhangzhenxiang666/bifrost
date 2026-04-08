# Bifrost 架构

## 概述

LLM 代理服务 (mapping service)，进行协议转换 (OpenAI ↔ Qwen/Anthropic)。

- **结构**: CLI (`src/`) + Server (`bifrost-server/`)
- **技术**: Rust + Axum + Tokio + Reqwest

---

## 核心概念

### Adapter (`adapter/`)

`Adapter` trait 定义请求/响应转换：

```rust
async fn transform_request(context: RequestContext<'_>) -> RequestTransform
async fn transform_response(body, status, headers) -> ResponseTransform
async fn transform_stream_chunk(chunk, event, provider_config) -> StreamChunkTransform
```

**内置适配器**: `PassthroughAdapter` | `OpenAIToQwenAdapter` | `AnthropicToOpenAIAdapter` | `AnthropicToQwenAdapter`

**Adapter Chain (OnionExecutor)**: 正向 A→B→C→Upstream，反向 Upstream→C→B→A

### ProviderRegistry (`provider/registry.rs`)

- 管理 provider 配置
- `build_executor(provider_id)` 构建 adapter chain
- HTTP 客户端 (600s 超时, 可选代理)

### Model 格式

`provider@model` (如 `qwen-code@coder-model`)

### Endpoint

`OpenAI` | `Anthropic`

---

## HTTP 路由

| 路由 | 端点 |
|------|------|
| `POST /openai/chat/completions` | OpenAI 兼容 |
| `POST /openai/v1/chat/completions`| OpenAI 兼容 |
| `POST /anthropic/v1/messages` | Anthropic 兼容 |
| `POST /anthropic/messages` | Anthropic 兼容 |

---

## 配置 (`~/.bifrost/config.toml`)

---
