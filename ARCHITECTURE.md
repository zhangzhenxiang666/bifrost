# Architecture

`bifrost` is a Rust workspace for proxying LLM requests between OpenAI-compatible and Anthropic-compatible protocols. It acts as a mapping service: routes accept one client protocol, providers declare their upstream endpoint protocol, and adapters convert requests, responses, and streaming chunks when the two protocols differ.

## Crate Responsibilities

- The root `bifrost` crate is the CLI. It loads and validates configuration, manages the local server process, prints status and usage information, handles upgrades, and provides operational commands.
- `bifrost-server` is the HTTP service. It owns Axum routing, request handling, provider lookup, adapter chain execution, upstream HTTP calls, SSE processing, middleware, and global server state.
- `bifrost-shared` owns cross-crate contracts. Shared config types, error helpers, usage records, and reusable utility types belong here.

Keep crate boundaries strict. CLI behavior should stay in the root crate, server execution should stay in `bifrost-server`, and dependency-light shared contracts should stay in `bifrost-shared`.

## Core Concepts

### Providers

Providers are configured in `~/.bifrost/config.toml`. Each provider declares:

- `base_url`
- `api_key`
- `endpoint`, currently `openai` or `anthropic`
- optional headers, body fields, model-specific overrides, excluded headers, retry settings, and body transformation policy

`ProviderRegistry` loads provider configuration, keeps aliases, builds the shared HTTP client, and creates adapter chains for a selected provider and route.

### Models And Aliases

The normal model selector format is `provider@model`, for example `openai@gpt-4o`.

Aliases can map a user-facing model name to a provider target. Complex aliases can also attach extra headers or body fields for the selected route.

### Adapters

`Adapter` is the protocol transformation boundary:

```rust
async fn transform_request(context: RequestContext) -> Result<RequestTransform, Self::Error>
async fn transform_response(context: ResponseContext<'_>) -> Result<ResponseTransform, Self::Error>
async fn transform_stream_chunk(context: StreamChunkContext<'_>) -> Result<StreamChunkTransform, Self::Error>
```

Built-in adapters are selected internally from the route endpoint and provider endpoint:

- `PassthroughAdapter`
- `OpenAIToAnthropicAdapter`
- `AnthropicToOpenAIAdapter`
- `ResponsesToChatAdapter`

Users configure providers, not adapters. Keep protocol conversion behavior in `bifrost-server/src/adapter/` and shared converter helpers under `bifrost-server/src/adapter/converter/`.

### Adapter Chain

`OnionExecutor` applies adapters in request order before the upstream call, then applies them in reverse order for upstream responses and stream chunks.

For example, an OpenAI Responses route targeting an Anthropic provider uses:

1. `ResponsesToChatAdapter`
2. `OpenAIToAnthropicAdapter`
3. upstream Anthropic request
4. reverse response or stream conversion

## HTTP Routes

| Route | Client protocol |
| --- | --- |
| `POST /openai/chat/completions` | OpenAI Chat Completions |
| `POST /openai/v1/chat/completions` | OpenAI Chat Completions |
| `POST /openai/responses` | OpenAI Responses |
| `POST /openai/v1/responses` | OpenAI Responses |
| `POST /anthropic/messages` | Anthropic Messages |
| `POST /anthropic/v1/messages` | Anthropic Messages |
| `GET /status` | Server status |

## Request Flow

1. The CLI starts the server with a loaded `Config`.
2. `bifrost-server` builds `ProviderRegistry`, stores `AppState`, configures CORS and request logging, then registers Axum routes.
3. A route handler receives a client request and determines the route endpoint.
4. The handler resolves the model target, provider, and model name from `provider@model` or alias configuration.
5. `ProviderRegistry::build_executor` selects the adapter chain from `(route endpoint, provider endpoint)`.
6. `OnionExecutor` transforms the outgoing request through each adapter.
7. The provider HTTP client sends the upstream request with configured timeout, retry, proxy, headers, and body settings.
8. Non-streaming responses are transformed back through the adapter chain in reverse.
9. Streaming responses are parsed as SSE and each chunk is transformed back through the adapter chain in reverse.
10. The route returns the converted response to the client protocol.

The route, provider, and adapter boundaries are intentional. Avoid pushing provider lookup into converters, protocol conversion into route handlers, or CLI concerns into the server crate.

## Where Changes Usually Go

- CLI commands, process management, config checks, upgrades, status printing, or usage display: `src/`.
- Shared config schema, usage records, shared errors, or reusable cross-crate types: `bifrost-shared/src/`.
- HTTP route wiring or request handler orchestration: `bifrost-server/src/routes/`.
- Provider lookup, aliases, adapter selection, retry settings, or upstream HTTP behavior: `bifrost-server/src/provider/`.
- Protocol-specific request, response, or streaming conversion: `bifrost-server/src/adapter/`.
- SSE parsing and formatting behavior: `bifrost-server/src/sse.rs` and stream converter modules.
- Request logging or cross-cutting HTTP behavior: `bifrost-server/src/middleware/`.

When a change touches multiple crates, update shared contracts first, then server behavior, then CLI presentation.

## Validation Strategy

For ordinary development, run focused crate-level checks for the crate you changed:

```bash
cargo test -p bifrost-server <test_name>
cargo check -p bifrost-server
cargo check -p bifrost-shared
```

For non-trivial Rust changes, finish with:

```bash
cargo clippy --workspace
cargo fmt --all
```

For documentation-only changes, no build or test command is required unless the documentation includes generated examples or checked snippets.
