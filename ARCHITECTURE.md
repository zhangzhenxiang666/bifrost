# Bifrost 架构

## 概述

LLM 代理服务 (mapping service)，进行协议转换 (OpenAI ↔ Anthropic)。

- **结构**: CLI (`src/`) + Server (`bifrost-server/`)
- **技术**: Rust + Axum + Tokio + Reqwest

---

## 核心概念

### Adapter (`adapter/`)

`Adapter` trait 定义请求/响应转换：

```rust
async fn transform_request(context: RequestContext) -> Result<RequestTransform, Self::Error>
async fn transform_response(context: ResponseContext<'_>) -> Result<ResponseTransform, Self::Error>
async fn transform_stream_chunk(context: StreamChunkContext<'_>) -> Result<StreamChunkTransform, Self::Error>
```

**内置适配器** (由系统内部动态创建): `PassthroughAdapter` | `AnthropicToOpenAIAdapter` | `ResponsesToChatAdapter`

> 用户无需配置适配器，系统会根据 Provider 的 `endpoint` 类型自动选择合适的适配器。

**Adapter Chain (OnionExecutor)**: 正向 A→B→C→Upstream，反向 Upstream→C→B→A

### ProviderRegistry (`provider/registry.rs`)

- 管理 provider 配置
- `build_executor(provider_id)` 构建 adapter chain
- HTTP 客户端 (600s 超时, 可选代理)

### Model 格式

`provider@model` (如 `openai@gpt-4o`)

### Endpoint

`OpenAI` | `Anthropic`

---

## HTTP 路由

| 路由 | 端点 |
|------|------|
| `POST /openai/chat/completions` | OpenAI 兼容 |
| `POST /openai/v1/chat/completions`| OpenAI 兼容 |
| `POST /openai/responses` | OpenAI Responses API → Chat 转换 |
| `POST /openai/v1/responses` | OpenAI Responses API → Chat 转换 |
| `POST /anthropic/v1/messages` | Anthropic 兼容 |
| `POST /anthropic/messages` | Anthropic 兼容 |

---

## 配置 (`~/.bifrost/config.toml`)

---
