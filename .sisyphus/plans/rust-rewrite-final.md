# Rust 重写 LLM Gateway - 最终版

> **版本**: Final (2026-03-01)  
> **状态**: 准备执行  
> **关键决策**: ✅ 使用 async-trait | ✅ axum Sse | ✅ 响应头完整透传

## TL;DR

> **核心目标**: 将 Python FastAPI LLM Gateway 重写为 Rust Axum 实现
> 
> **关键设计决策**:
> 1. ✅ **使用 async-trait** - 简化异步代码（用户确认接受）
> 2. ✅ **axum::response::sse::Sse** - axum 内置 SSE 支持
> 3. ✅ **响应头完整透传** - 保留所有上游 headers（rate limit 等）
> 4. ✅ **结构化返回类型** - `RequestTransform`/`ResponseTransform`
> 5. ✅ **统一路由处理 SSE** - 根据 `stream: bool` 返回不同类型
> 
> **技术栈**: 
> - Axum 0.8.8 + Tokio 1.49.0 + reqwest 0.12
> - **async-trait 0.1** ✅ (用户接受使用)
> - thiserror + anyhow
> - tracing + tracing-subscriber
> - **axum::response::sse::Sse** ✅
> 
> **交付物**:
> - 完整的 Rust LLM Gateway
> - 配置系统（TOML + OneOrMany）
> - 适配器链（洋葱模型 + **响应头透传**）
> - OpenAI/Anthropic 兼容路由（**统一 SSE 处理**）
> - 完整的错误处理和日志
> 
> **任务数**: 24 任务
> **并行执行**: 4 个 Wave，最多 8 任务并行 (Wave 2)
> **关键路径**: T1 → T3 → T5 → T7 → T8 → T15 → T21 → F1-F4

---

## Core Design

### 1. Adapter Trait (使用 async-trait) ✅

```rust
use async_trait::async_trait;

#[async_trait]
pub trait Adapter: Send + Sync {
    type Error: std::error::Error + Send + Sync;
    
    async fn transform_request(
        &self,
        body: serde_json::Value,
        url: &str,
        headers: &http::HeaderMap,
    ) -> Result<RequestTransform, Self::Error>;
    
    async fn transform_response(
        &self,
        body: serde_json::Value,
        status: http::StatusCode,
        headers: &http::HeaderMap,
    ) -> Result<ResponseTransform, Self::Error>;
    
    async fn transform_stream_chunk(
        &self,
        chunk: serde_json::Value,
    ) -> Result<StreamChunkTransform, Self::Error>;
}
```

### 2. 结构化返回类型

```rust
pub struct RequestTransform {
    pub body: serde_json::Value,
    pub url: Option<String>,
    pub headers: Option<http::HeaderMap>,
}

pub struct ResponseTransform {
    pub body: serde_json::Value,
    pub status: Option<http::StatusCode>,
    pub headers: Option<http::HeaderMap>,
}

pub struct StreamChunkTransform {
    pub data: serde_json::Value,
    pub event: Option<String>,
}
```

### 3. GatewayResponse (使用 axum::Sse) ✅

```rust
use axum::response::sse::{Event, Sse};
use futures::stream::Stream;
use std::convert::Infallible;
use std::pin::Pin;

pub type SSEStream = Sse<Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>>>;

pub enum GatewayResponse {
    Json(axum::Json<serde_json::Value>),
    Sse(SSEStream),
}

impl IntoResponse for GatewayResponse {
    fn into_response(self) -> Response {
        match self {
            GatewayResponse::Json(json) => json.into_response(),
            GatewayResponse::Sse(sse) => sse.into_response(),
        }
    }
}
```

### 4. 响应头透传策略 ✅

**原则**: 默认透传所有上游响应头，只修改必要的头

**透传的头** (保留所有有价值的信息):
- ✅ `x-ratelimit-limit` - 速率限制
- ✅ `x-ratelimit-remaining` - 剩余请求数
- ✅ `x-ratelimit-reset` - 重置时间
- ✅ `retry-after` - 重试等待
- ✅ `x-request-id` - 请求追踪
- ✅ 其他所有 provider 返回的头

**重新计算的头**:
- ❌ `content-length` - 根据实际响应体
- ❌ `transfer-encoding` - 由 axum 处理

**实现**:
```rust
// 执行器中透传 headers
let upstream_response = client.post(...).send().await?;
let mut response_headers = HeaderMap::new();

// 透传所有上游 headers
for (key, value) in upstream_response.headers() {
    if key != "content-length" && key != "transfer-encoding" {
        response_headers.insert(key, value.clone());
    }
}

// 应用适配器修改的 headers
if let Some(adapter_headers) = transform.headers {
    for (key, value) in adapter_headers {
        response_headers.insert(key, value);
    }
}
```

### 5. 统一路由处理

```rust
#[router.post("/chat/completions")]
async fn chat_completions(
    State(state): State<AppState>,
    Json(mut body): Json<serde_json::Value>,
) -> Result<GatewayResponse> {
    let is_stream = body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);
    let response = state.executor.execute(body).await?;
    
    Ok(if is_stream {
        GatewayResponse::Sse(convert_to_sse(response))
    } else {
        GatewayResponse::Json(axum::Json(response))
    })
}
```

---

## Context

### 原始请求
使用 Rust 重写 Python LLM Gateway 项目，主要变更：
1. 移除自定义适配器支持（Rust 编译型语言限制）
2. 移除 models 字段的 adapter 支持（简化配置）
3. 为 provider 和 models 添加 headers/body 字段
4. 使用 Axum + Tokio 作为技术栈

### 用户反馈决策 (2026-03-01)

| 决策点 | 最终选择 | 理由 |
|--------|----------|------|
| async-trait | ✅ **使用** | 项目前期，简化开发 |
| SSE 类型 | ✅ **axum::Sse** | axum 内置支持，文档完善 |
| 响应头 | ✅ **完整透传** | 保留 rate limit 等信息，客户端无感切换 |
| Future 返回 | ✅ **async fn** | async-trait 自动处理 |
| 路由分离 | ❌ **统一处理** | 根据 `stream: bool` 返回不同类型 |

### 技术栈确认

| 组件 | 版本 | 备注 |
|------|------|------|
| axum | 0.8.8 | 用户指定 |
| tokio | 1.49.0 | 用户指定 |
| reqwest | 0.12 | 用户指定 |
| **async-trait** | **0.1** | ✅ **用户接受使用** |
| serde | 1.0 | 序列化 |
| serde_json | 1.0 | JSON 处理 |
| thiserror | 2.0 | 错误定义 |
| anyhow | 1.0 | 错误处理 |
| tracing | 0.1 | 日志追踪 |
| tracing-subscriber | 0.3 | 日志输出 |
| tower-http | 0.6 | 中间件 |
| validator | 0.18 | 配置验证 |
| futures | 0.3 | Stream 支持 |
| http | 1.0 | HTTP 类型 |

---

## Work Objectives

### 核心目标
实现与 Python 版本功能对等的 Rust LLM Gateway，利用 Rust 的类型安全和性能优势，**保持客户端无感切换**（响应头透传）

### 具体交付物
- `src/config/` - 配置加载、解析、验证
- `src/adapter/` - 适配器 trait (async-trait)、执行器、内置适配器
- `src/provider/` - provider 管理、HTTP 客户端
- `src/routes/` - OpenAI/Anthropic 路由（统一 SSE）
- `src/types/` - 共享类型（Transform 类型）
- `src/error.rs` - 分层错误
- `tests/` - 单元和集成测试
- `Cargo.toml` (包含 async-trait)

### 完成定义
- [ ] 所有任务完成并通过测试
- [ ] 配置加载成功
- [ ] 请求转发到真实 LLM provider 成功
- [ ] **响应头正确透传**（x-ratelimit-* 等）
- [ ] 流式和非流式请求均正常工作
- [ ] 错误处理覆盖所有场景
- [ ] tracing 日志输出正常

### Must Have
- 配置系统支持 OneOrMany
- 适配器链正确实现洋葱模型
- **响应头完整透传**
- 统一路由根据 `stream: bool` 返回正确类型
- 错误类型分层清晰

### Must NOT Have (Guardrails)
- ❌ 不包含自定义适配器动态加载
- ❌ 不包含 models[].adapter 字段
- ❌ 不分离 SSE 路由（统一处理）
- ❌ 不使用运行时反射
- ❌ 不修改 HTTP method（都是 POST）

---

## Execution Strategy

### 并行执行 Waves

```
Wave 1 (基础架构 - 6 任务):
├── T1: 项目脚手架 + Cargo.toml [quick]
├── T2: 错误类型定义 [quick]
├── T3: 配置结构 (OneOrMany) [unspecified-high]
├── T4: 类型定义 (Newtype + Transform) [quick]
├── T5: 配置加载和验证 [unspecified-high]
└── T6: tracing 日志配置 [quick]

Wave 2 (核心模块 - 8 任务):
├── T7: Adapter trait (async-trait) [unspecified-high]
├── T8: 洋葱模型执行器 + 响应头透传 [deep]
├── T9: GatewayResponse enum [quick]
├── T10: Passthrough 适配器 [quick]
├── T11: OpenAIToQwen 适配器 [unspecified-high]
├── T12: HTTP 客户端 (reqwest) [quick]
├── T13: Provider 注册表 [unspecified-high]
└── T14: SSE 转换器 [quick]

Wave 3 (路由和集成 - 5 任务):
├── T15: OpenAI 路由 (统一 SSE) [deep]
├── T16: Anthropic 路由 (统一 SSE) [deep]
├── T17: 健康检查和模型列表 [quick]
├── T18: CORS 和中间件 [quick]
└── T19: main.rs 应用组装 [quick]

Wave 4 (测试和验证 - 4 任务):
├── T20: 单元测试编写 [deep]
├── T21: 集成测试编写 [deep]
├── T22: 端到端手动测试 [unspecified-high]
└── T23: 性能基准测试 [quick]

Wave FINAL (独立审查 - 4 任务并行):
├── F1: 计划合规审计 (oracle)
├── F2: 代码质量审查 (unspecified-high)
├── F3: 真实手动 QA (unspecified-high)
└── F4: 范围保真检查 (deep)

关键路径: T1 → T3 → T5 → T7 → T8 → T15 → T21 → F1-F4
最大并发: 8 任务 (Wave 2)
```

---

## TODOs

> 注意：关键任务已根据用户最终决策更新
> - T7: 使用 `#[async_trait]` 宏
> - T8: 添加响应头透传逻辑
> - T9: 使用 `axum::response::sse::Sse`
> - T15-T16: 统一 SSE 处理

- [x] 1. 项目脚手架 + Cargo.toml 依赖配置

  **做什么**:
  - 更新 `Cargo.toml` 添加所有必需依赖
  - **包含 async-trait 0.1** ✅
  - 创建基础目录结构
  - 创建 `src/lib.rs` 和 `src/main.rs`

  **必须不做**:
  - 不实现任何业务逻辑

  **推荐 Agent Profile**:
  - **Category**: `quick`
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 1
  - **阻塞**: T18, T19
  - **被阻塞**: 无

  **验收标准**:
  - [ ] `cargo check` 通过
  - [ ] Cargo.toml 包含 async-trait
  - [ ] 目录结构创建完成

  **QA 场景**:
  ```
  场景：验证项目脚手架
    工具：Bash (cargo check)
    步骤:
      1. 运行 `cargo check`
      2. 检查无编译错误
      3. 检查 Cargo.toml 包含 async-trait
    预期结果：编译成功
    证据：.sisyphus/evidence/task-1-scaffold.txt
  ```

  **提交**: YES (与 T2, T4, T6 分组)
  - 消息：`chore(project): setup project scaffolding`
  - 文件：`Cargo.toml`, `src/lib.rs`, `src/main.rs`
  - 预提交：`cargo check`

---

- [x] 2. 错误类型定义 (thiserror)

  **做什么**:
  - 创建 `src/error.rs`
  - 定义分层错误类型
  - 实现 `IntoResponse` trait

  **推荐 Agent Profile**:
  - **Category**: `quick`
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 1
  - **阻塞**: T7-T23
  - **被阻塞**: 无

  **验收标准**:
  - [ ] 错误类型定义完整
  - [ ] 实现 `IntoResponse`
  - [ ] 实现 `From` trait

  **QA 场景**:
  ```
  场景：验证错误类型转换
    工具：Bash (cargo test)
    步骤:
      1. 编写错误转换测试
      2. 运行 `cargo test error`
    预期结果：所有转换测试通过
    证据：.sisyphus/evidence/task-2-error-tests.txt
  ```

  **提交**: YES (与 T1, T4, T6 分组)
  - 消息：`feat(error): define layered error types`
  - 文件：`src/error.rs`
  - 预提交：`cargo test error`

---

- [x] 3. 配置结构设计 (OneOrMany)

  **做什么**:
  - 创建 `src/config/mod.rs`
  - 实现 `OneOrMany<T>` 结构
  - 定义配置结构体

  **推荐 Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 1
  - **阻塞**: T5, T13, T19
  - **被阻塞**: 无

  **验收标准**:
  - [ ] `OneOrMany<String>` 可反序列化字符串和数组
  - [ ] 配置结构体定义完整

  **QA 场景**:
  ```
  场景：验证 OneOrMany 反序列化
    工具：Bash (cargo test)
    步骤:
      1. 测试 "adapter" 解析
      2. 测试 ["a", "b"] 解析
      3. 断言结果都是 Vec
    预期结果：两种格式都正确解析
    证据：.sisyphus/evidence/task-3-one-or-many-test.txt
  ```

  **提交**: YES (单独提交)
  - 消息：`feat(config): implement OneOrMany deserializer`
  - 文件：`src/config/mod.rs`
  - 预提交：`cargo test config`

---

- [x] 4. 类型定义 (Newtype + Transform)

  **做什么**:
  - 创建 `src/types/mod.rs`
  - 定义 Newtype 类型：`ApiKey`, `ModelId`, `ProviderId`, `AdapterId`, `RequestId`
  - 定义 `RequestTransform`, `ResponseTransform`, `StreamChunkTransform`

  **推荐 Agent Profile**:
  - **Category**: `quick`
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 1
  - **阻塞**: T7, T9, T14
  - **被阻塞**: 无

  **验收标准**:
  - [ ] Newtype 类型定义完成
  - [ ] Transform 类型定义完成
  - [ ] `ApiKey.mask()` 方法正确

  **QA 场景**:
  ```
  场景：验证 ApiKey 安全性
    工具：Bash (cargo test)
    步骤:
      1. 创建 ApiKey
      2. 调用 .mask()
      3. 断言结果隐藏敏感信息
    预期结果：mask 返回 "sk-ve****2345" 格式
    证据：.sisyphus/evidence/task-4-apikey-mask.txt
  ```

  **提交**: YES (与 T1, T2, T6 分组)
  - 消息：`feat(types): define Newtype and Transform types`
  - 文件：`src/types/mod.rs`
  - 预提交：`cargo test types`

---

- [x] 5. 配置加载和验证

  **做什么**:
  - 创建 `src/config/loader.rs` 和 `validator.rs`
  - 实现 `Config::from_file()` 和 `Config::validate()`

  **推荐 Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 1
  - **阻塞**: T13, T19
  - **被阻塞**: T3

  **验收标准**:
  - [ ] `Config::from_file()` 正确解析
  - [ ] `Config::validate()` 检查 adapter 存在

  **QA 场景**:
  ```
  场景：验证配置加载
    工具：Bash (cargo test)
    步骤:
      1. 创建有效和无效配置文件
      2. 加载并验证
      3. 断言错误信息正确
    预期结果：有效配置通过，无效配置返回错误
    证据：.sisyphus/evidence/task-5-config-load.txt
  ```

  **提交**: YES (单独提交)
  - 消息：`feat(config): implement config loader and validator`
  - 文件：`src/config/loader.rs`, `src/config/validator.rs`
  - 预提交：`cargo test config`

---

- [x] 6. tracing 日志配置

  **做什么**:
  - 创建 `src/utils/logging.rs`
  - 配置 `tracing-subscriber`
  - 实现 `init_logging()` 函数

  **推荐 Agent Profile**:
  - **Category**: `quick`
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 1
  - **阻塞**: 无
  - **被阻塞**: 无

  **验收标准**:
  - [ ] `init_logging()` 正确配置
  - [ ] `RUST_LOG=debug` 可控制级别

  **QA 场景**:
  ```
  场景：验证日志输出
    工具：Bash (cargo run)
    步骤:
      1. 设置 RUST_LOG=debug
      2. 运行 cargo run
      3. 检查日志输出
    预期结果：看到带时间戳和模块的日志
    证据：.sisyphus/evidence/task-6-logging.txt
  ```

  **提交**: YES (与 T1, T2, T4 分组)
  - 消息：`feat(logging): configure tracing`
  - 文件：`src/utils/logging.rs`, `src/utils/mod.rs`
  - 预提交：`RUST_LOG=debug cargo run`

---

- [x] 7. Adapter trait (使用 async-trait) ✅

  **做什么**:
  - 创建 `src/adapter/mod.rs`, `src/adapter/trait.rs`, `src/adapter/context.rs`
  - 定义 `Adapter` trait (使用 `#[async_trait]`):
    ```rust
    #[async_trait]
    pub trait Adapter: Send + Sync {
        type Error: std::error::Error + Send + Sync;
        
        async fn transform_request(...) -> Result<RequestTransform, Self::Error>;
        async fn transform_response(...) -> Result<ResponseTransform, Self::Error>;
        async fn transform_stream_chunk(...) -> Result<StreamChunkTransform, Self::Error>;
    }
    ```
  - 定义 `RequestContext` 和 `ResponseContext`

  **必须不做**:
  - 不使用 `Pin<Box<dyn Future>>`（async-trait 自动处理）

  **推荐 Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: NO
  - **并行组**: Wave 2 起始
  - **阻塞**: T8-T11, T15-T16
  - **被阻塞**: T2, T4, T6

  **验收标准**:
  - [ ] `Adapter` trait 定义完整（使用 `#[async_trait]`）
  - [ ] `cargo doc` 生成无警告

  **QA 场景**:
  ```
  场景：验证 Adapter trait 编译
    工具：Bash (cargo check)
    步骤:
      1. 运行 cargo check
      2. 运行 cargo doc --no-deps
    预期结果：编译和文档生成成功
    证据：.sisyphus/evidence/task-7-adapter-trait.txt
  ```

  **提交**: YES (单独提交)
  - 消息：`feat(adapter): define Adapter trait with async-trait`
  - 文件：`src/adapter/trait.rs`, `src/adapter/context.rs`, `src/adapter/mod.rs`
  - 预提交：`cargo doc --no-deps`

---

- [x] 8. 洋葱模型执行器 + 响应头透传 ✅

  **做什么**:
  - 创建 `src/adapter/chain.rs`
  - 实现 `OnionExecutor` 结构
  - 实现请求正向和响应反向执行
  - **关键**: 实现响应头透传逻辑

  **必须不做**:
  - 不透传 `content-length` 和 `transfer-encoding`

  **推荐 Agent Profile**:
  - **Category**: `deep`
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: NO
  - **并行组**: Wave 2 关键路径
  - **阻塞**: T15-T16, T20-T22
  - **被阻塞**: T7

  **验收标准**:
  - [ ] 请求正向执行适配器链
  - [ ] 响应反向执行适配器链
  - [ ] **响应头正确透传**（x-ratelimit-* 等）
  - [ ] `content-length` 正确重新计算

  **QA 场景**:
  ```
  场景：验证洋葱模型执行顺序
    工具：Bash (cargo test)
    步骤:
      1. 创建 3 个 Mock 适配器
      2. 执行请求
      3. 断言请求顺序：A → B → C
      4. 断言响应顺序：C → B → A
    预期结果：执行顺序符合洋葱模型
    证据：.sisyphus/evidence/task-8-onion-order.txt
  
  场景：验证响应头透传
    工具：Bash (cargo test + curl -v)
    步骤:
      1. mock 服务器返回 x-ratelimit-limit 头
      2. 发送请求到网关
      3. 检查响应包含 x-ratelimit-limit
      4. 断言 content-length 正确
    预期结果：上游 headers 正确透传
    证据：.sisyphus/evidence/task-8-headers-passthrough.txt
  ```

  **提交**: YES (单独提交)
  - 消息：`feat(adapter): implement OnionExecutor with header passthrough`
  - 文件：`src/adapter/chain.rs`
  - 预提交：`cargo test chain`

---

- [ ] 9. GatewayResponse enum (axum::Sse) ✅

  **做什么**:
  - 创建 `src/types/response.rs`
  - 定义 `GatewayResponse` enum:
    ```rust
    pub type SSEStream = Sse<Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>>>;
    
    pub enum GatewayResponse {
        Json(axum::Json<serde_json::Value>),
        Sse(SSEStream),
    }
    ```
  - 实现 `IntoResponse` trait

  **必须不做**:
  - 不在 enum 变体中使用 `impl Trait`

  **推荐 Agent Profile**:
  - **Category**: `quick`
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 2
  - **阻塞**: T15-T16, T19
  - **被阻塞**: T2, T4

  **验收标准**:
  - [ ] `GatewayResponse` enum 定义正确
  - [ ] 使用 `axum::response::sse::Sse`
  - [ ] 实现 `IntoResponse`

  **QA 场景**:
  ```
  场景：验证 GatewayResponse 类型
    工具：Bash (cargo test)
    步骤:
      1. 创建 Json 变体
      2. 创建 Sse 变体（使用 mock stream）
      3. 调用 into_response()
    预期结果：两种变体都正确转换为 Response
    证据：.sisyphus/evidence/task-9-gateway-response.txt
  ```

  **提交**: YES (单独提交)
  - 消息：`feat(types): define GatewayResponse enum with axum Sse`
  - 文件：`src/types/response.rs`
  - 预提交：`cargo test response`

---

- [ ] 10. Passthrough 适配器实现

  **做什么**:
  - 创建 `src/adapter/builtin/passthrough.rs`
  - 实现 `PassthroughAdapter`
  - 实现 `Adapter` trait（返回不修改的数据）

  **推荐 Agent Profile**:
  - **Category**: `quick`
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 2
  - **阻塞**: 无
  - **被阻塞**: T7

  **验收标准**:
  - [ ] 实现 Adapter trait
  - [ ] 请求和响应都不修改

  **QA 场景**:
  ```
  场景：验证 Passthrough 不修改数据
    工具：Bash (cargo test)
    步骤:
      1. 创建请求上下文
      2. 执行 transform_request
      3. 断言 body 不变
    预期结果：数据完全不变
    证据：.sisyphus/evidence/task-10-passthrough.txt
  ```

  **提交**: YES (与 T12, T14 分组)
  - 消息：`feat(adapter): implement PassthroughAdapter`
  - 文件：`src/adapter/builtin/passthrough.rs`, `src/adapter/builtin/mod.rs`
  - 预提交：`cargo test passthrough`

---

- [ ] 11. OpenAIToQwen 适配器实现

  **做什么**:
  - 创建 `src/adapter/builtin/openai_to_qwen.rs`
  - 实现 `OpenAIToQwenAdapter`
  - 转换 OpenAI 格式 → Qwen 格式

  **推荐 Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 2
  - **阻塞**: T15
  - **被阻塞**: T7

  **验收标准**:
  - [ ] 正确转换 OpenAI messages → Qwen messages
  - [ ] 添加必要的 Qwen headers

  **QA 场景**:
  ```
  场景：验证 OpenAI 到 Qwen 格式转换
    工具：Bash (cargo test)
    步骤:
      1. 创建 OpenAI 格式请求
      2. 执行 transform_request
      3. 断言转换为 Qwen 格式
    预期结果：格式正确转换
    证据：.sisyphus/evidence/task-11-qwen-transform.txt
  ```

  **提交**: YES (单独提交)
  - 消息：`feat(adapter): implement OpenAIToQwenAdapter`
  - 文件：`src/adapter/builtin/openai_to_qwen.rs`
  - 预提交：`cargo test openai_to_qwen`

---

- [ ] 12. HTTP 客户端封装 (reqwest)

  **做什么**:
  - 创建 `src/provider/client.rs`
  - 实现 `HttpClient` 结构
  - 实现 `send_request` 和 `send_stream`

  **推荐 Agent Profile**:
  - **Category**: `quick`
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 2
  - **阻塞**: T13, T15-T16
  - **被阻塞**: T2, T6

  **验收标准**:
  - [ ] 支持非流式 POST 请求
  - [ ] 支持流式请求返回 Stream
  - [ ] 配置超时 600 秒

  **QA 场景**:
  ```
  场景：验证 HTTP 客户端发送请求
    工具：Bash (cargo test)
    步骤:
      1. 启动 wiremock 测试服务器
      2. 配置 mock 响应
      3. 调用 send_request
      4. 断言收到正确响应
    预期结果：请求成功发送并接收
    证据：.sisyphus/evidence/task-12-http-client.txt
  ```

  **提交**: YES (与 T10, T14 分组)
  - 消息：`feat(provider): implement HttpClient with reqwest`
  - 文件：`src/provider/client.rs`
  - 预提交：`cargo test client`

---

- [ ] 13. Provider 注册表和管理

  **做什么**:
  - 创建 `src/provider/registry.rs`
  - 实现 `ProviderRegistry` 结构
  - 合并 provider 和 model 的 headers/body

  **推荐 Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 2
  - **阻塞**: T15-T16
  - **被阻塞**: T3, T5, T12

  **验收标准**:
  - [ ] 从配置正确构建 provider
  - [ ] 正确合并 headers/body
  - [ ] 构建适配器链

  **QA 场景**:
  ```
  场景：验证 Provider 注册表
    工具：Bash (cargo test)
    步骤:
      1. 加载测试配置
      2. 调用 get("qwen-code")
      3. 断言返回正确的 base_url, api_key
    预期结果：provider 信息正确
    证据：.sisyphus/evidence/task-13-registry.txt
  ```

  **提交**: YES (单独提交)
  - 消息：`feat(provider): implement ProviderRegistry`
  - 文件：`src/provider/registry.rs`, `src/provider/mod.rs`
  - 预提交：`cargo test registry`

---

- [ ] 14. SSE 转换器实现

  **做什么**:
  - 创建 `src/utils/sse.rs`
  - 实现 `convert_to_sse(response: Value) -> SSEStream` 函数
  - 实现 `parse_sse_events(text: &str) -> Vec<Event>` 函数

  **推荐 Agent Profile**:
  - **Category**: `quick`
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 2
  - **阻塞**: T15-T16
  - **被阻塞**: T4, T8

  **验收标准**:
  - [ ] 正确解析 SSE 事件
  - [ ] 正确处理 [DONE] 标记
  - [ ] 保持连接活跃

  **QA 场景**:
  ```
  场景：验证 SSE 流式转换
    工具：Bash (cargo test)
    步骤:
      1. 创建 mock SSE 数据
      2. 调用 convert_to_sse
      3. 断言生成的 stream 正确
    预期结果：SSE 事件正确转换
    证据：.sisyphus/evidence/task-14-sse-convert.txt
  ```

  **提交**: YES (与 T10, T12 分组)
  - 消息：`feat(sse): implement SSE converter`
  - 文件：`src/utils/sse.rs`
  - 预提交：`cargo test sse`

---

- [ ] 15. OpenAI 兼容路由 (统一 SSE)

  **做什么**:
  - 创建 `src/routes/openai.rs`
  - 实现 `chat_completions` 处理函数:
    - 解析 `provider@model` 格式
    - 根据 `stream: bool` 返回 `GatewayResponse::Json` 或 `GatewayResponse::Sse`

  **必须不做**:
  - 不实现适配器逻辑
  - 不分离 SSE 路由

  **推荐 Agent Profile**:
  - **Category**: `deep`
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: NO
  - **并行组**: Wave 3 关键路径
  - **阻塞**: T19, T20-T22
  - **被阻塞**: T7-T9, T11-T14

  **验收标准**:
  - [ ] 正确解析 provider@model 格式
  - [ ] 根据 `stream: bool` 返回正确类型
  - [ ] 错误处理返回正确状态码

  **QA 场景**:
  ```
  场景：验证 OpenAI 路由非流式请求
    工具：Bash (curl)
    步骤:
      1. 发送 POST /v1/chat/completions
      2. Body: {"model": "qwen-code@...", "stream": false}
      3. 断言响应状态 200 且为 JSON
    预期结果：返回 OpenAI 格式 JSON 响应
    证据：.sisyphus/evidence/task-15-openai-route.txt
  
  场景：验证 OpenAI 路由流式请求
    工具：Bash (curl -N)
    步骤:
      1. 发送 POST /v1/chat/completions
      2. Body: {"model": "qwen-code@...", "stream": true}
      3. 断言收到 SSE 流
      4. 断言流以 [DONE] 结束
    预期结果：返回 SSE 流
    证据：.sisyphus/evidence/task-15-openai-stream.txt
  ```

  **提交**: YES (单独提交)
  - 消息：`feat(routes): implement OpenAI route with unified SSE`
  - 文件：`src/routes/openai.rs`, `src/routes/mod.rs`
  - 预提交：`cargo test routes`

---

- [ ] 16. Anthropic 兼容路由 (统一 SSE)

  **做什么**:
  - 创建 `src/routes/anthropic.rs`
  - 实现 `messages` 处理函数（类似 T15）

  **推荐 Agent Profile**:
  - **Category**: `deep`
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 3
  - **阻塞**: T19, T20-T22
  - **被阻塞**: T7-T9, T12-T14

  **验收标准**:
  - [ ] 正确解析 provider@model 格式
  - [ ] 根据 `stream: bool` 返回正确类型
  - [ ] 错误处理返回正确状态码

  **QA 场景**:
  ```
  场景：验证 Anthropic 路由非流式请求
    工具：Bash (curl)
    步骤:
      1. 发送 POST /v1/messages
      2. Body: {"model": "qwen-code@..."}
      3. 断言响应符合 Anthropic 格式
    预期结果：返回正确响应
    证据：.sisyphus/evidence/task-16-anthropic-route.txt
  ```

  **提交**: YES (单独提交)
  - 消息：`feat(routes): implement Anthropic route with unified SSE`
  - 文件：`src/routes/anthropic.rs`
  - 预提交：`cargo test routes`

---

- [ ] 17. 健康检查和模型列表路由

  **做什么**:
  - 创建 `src/routes/health.rs`, `src/routes/models.rs`
  - 实现 `health_check` 和 `list_models`

  **推荐 Agent Profile**:
  - **Category**: `quick`
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 3
  - **阻塞**: T19
  - **被阻塞**: T13

  **验收标准**:
  - [ ] GET /health 返回 200
  - [ ] GET /v1/models 返回模型列表

  **QA 场景**:
  ```
  场景：验证健康检查路由
    工具：Bash (curl)
    步骤:
      1. 发送 GET /health
      2. 断言状态码 200
    预期结果：健康检查通过
    证据：.sisyphus/evidence/task-17-health.txt
  ```

  **提交**: YES (与 T18 分组)
  - 消息：`feat(routes): add health check and models list endpoints`
  - 文件：`src/routes/health.rs`, `src/routes/models.rs`
  - 预提交：`curl http://localhost:5564/health`

---

- [ ] 18. CORS 和中间件配置

  **做什么**:
  - 创建 `src/routes/middleware.rs`
  - 配置 CORS、tracing、超时中间件

  **推荐 Agent Profile**:
  - **Category**: `quick`
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 3
  - **阻塞**: T19
  - **被阻塞**: T1

  **验收标准**:
  - [ ] CORS headers 正确添加
  - [ ] 请求日志包含方法、路径、状态码

  **QA 场景**:
  ```
  场景：验证 CORS 中间件
    工具：Bash (curl)
    步骤:
      1. 发送 OPTIONS 预检请求
      2. 断言响应包含 CORS headers
    预期结果：CORS headers 正确
    证据：.sisyphus/evidence/task-18-cors.txt
  ```

  **提交**: YES (与 T17 分组)
  - 消息：`feat(middleware): configure CORS and tracing middleware`
  - 文件：`src/routes/middleware.rs`
  - 预提交：`curl -v -X OPTIONS http://localhost:5564/v1/chat/completions`

---

- [ ] 19. main.rs 应用组装

  **做什么**:
  - 更新 `src/main.rs`
  - 创建 `create_app()` 函数
  - 加载配置、验证、初始化 tracing、配置路由
  - 启动 Tokio 运行时

  **推荐 Agent Profile**:
  - **Category**: `quick`
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: NO
  - **并行组**: Wave 3 收尾
  - **阻塞**: T20-T23
  - **被阻塞**: T1, T3, T5, T15-T18

  **验收标准**:
  - [ ] `cargo run` 成功启动服务器
  - [ ] 监听端口 5564
  - [ ] 所有路由可访问

  **QA 场景**:
  ```
  场景：验证应用启动
    工具：Bash (cargo run)
    步骤:
      1. 运行 cargo run
      2. 检查输出包含 "listening on port 5564"
      3. 发送测试请求
    预期结果：服务器成功启动并响应
    证据：.sisyphus/evidence/task-19-app-start.txt
  ```

  **提交**: YES (单独提交)
  - 消息：`feat(main): assemble application and start server`
  - 文件：`src/main.rs`
  - 预提交：`cargo run &`

---

- [ ] 20. 单元测试编写

  **做什么**:
  - 为所有核心模块编写单元测试
  - 使用 `#[cfg(test)]` 模块
  - 使用 mockall 进行 mock

  **推荐 Agent Profile**:
  - **Category**: `deep`
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: NO
  - **并行组**: Wave 4 起始
  - **阻塞**: F1-F4
  - **被阻塞**: T7-T18

  **验收标准**:
  - [ ] 所有核心模块有单元测试
  - [ ] `cargo test` 通过率 100%
  - [ ] 测试覆盖率 > 80%

  **QA 场景**:
  ```
  场景：运行所有单元测试
    工具：Bash (cargo test)
    步骤:
      1. 运行 cargo test --lib
      2. 检查所有测试通过
    预期结果：所有测试通过
    证据：.sisyphus/evidence/task-20-unit-tests.txt
  ```

  **提交**: YES (单独提交)
  - 消息：`test(unit): add comprehensive unit tests`
  - 文件：`src/**/*.rs` (测试模块)
  - 预提交：`cargo test --lib`

---

- [ ] 21. 集成测试编写

  **做什么**:
  - 创建 `tests/integration/` 目录
  - 编写端到端测试
  - 使用 wiremock 模拟上游 provider
  - **测试响应头透传**

  **推荐 Agent Profile**:
  - **Category**: `deep`
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 4
  - **阻塞**: F1-F4
  - **被阻塞**: T15-T19

  **验收标准**:
  - [ ] 集成测试覆盖所有路由
  - [ ] 使用 mock 服务器
  - [ ] 测试流式和非流式
  - [ ] **测试响应头透传**

  **QA 场景**:
  ```
  场景：运行集成测试
    工具：Bash (cargo test)
    步骤:
      1. 运行 cargo test --test '*'
      2. 检查所有集成测试通过
    预期结果：所有集成测试通过
    证据：.sisyphus/evidence/task-21-integration-tests.txt
  ```

  **提交**: YES (单独提交)
  - 消息：`test(integration): add end-to-end integration tests`
  - 文件：`tests/integration/*.rs`
  - 预提交：`cargo test --test '*'`

---

- [ ] 22. 端到端手动测试

  **做什么**:
  - 使用真实配置启动服务器
  - 测试所有场景（流式和非流式）
  - **验证响应头透传**
  - 记录所有测试结果

  **推荐 Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 4
  - **阻塞**: F1-F4
  - **被阻塞**: T19

  **验收标准**:
  - [ ] 所有场景测试通过
  - [ ] 响应格式正确
  - [ ] **响应头正确透传**
  - [ ] 错误处理正确

  **QA 场景**:
  ```
  场景：端到端 OpenAI 流式测试
    工具：Bash (curl -N -v)
    步骤:
      1. curl -N -v -X POST http://localhost:5564/v1/chat/completions
      2. Body: {"model": "qwen-code@...", "stream": true}
      3. 断言收到 SSE 流
      4. 断言响应头包含 x-ratelimit-* (如果有)
      5. 断言流以 [DONE] 结束
    预期结果：流式响应正确，headers 透传
    证据：.sisyphus/evidence/task-22-e2e-stream.txt
  ```

  **提交**: YES (单独提交)
  - 消息：`test(e2e): perform manual end-to-end testing`
  - 文件：`tests/e2e/README.md`
  - 预提交：手动验证

---

- [ ] 23. 性能基准测试

  **做什么**:
  - 创建 `benches/` 目录
  - 使用 criterion 进行基准测试

  **推荐 Agent Profile**:
  - **Category**: `quick`
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: NO
  - **并行组**: Wave 4 收尾
  - **阻塞**: F1-F4
  - **被阻塞**: T19

  **验收标准**:
  - [ ] 基准测试可运行
  - [ ] 生成 HTML 报告

  **QA 场景**:
  ```
  场景：运行基准测试
    工具：Bash (cargo bench)
    步骤:
      1. 运行 cargo bench
      2. 检查生成 HTML 报告
    预期结果：基准测试完成
    证据：.sisyphus/evidence/task-23-benchmarks.txt
  ```

  **提交**: YES (单独提交)
  - 消息：`perf(bench): add criterion benchmarks`
  - 文件：`benches/*.rs`
  - 预提交：`cargo bench`

---

## Final Verification Wave

> 4 个审查 Agent 并行执行，全部必须 APPROVE

- [ ] F1. **计划合规审计** — `oracle`
- [ ] F2. **代码质量审查** — `unspecified-high`
- [ ] F3. **真实手动 QA** — `unspecified-high`
- [ ] F4. **范围保真检查** — `deep`

---

## Commit Strategy

- **Wave 1**: `chore(project): setup scaffolding` — Cargo.toml, src/lib.rs, src/main.rs, src/error.rs, src/types/mod.rs, src/utils/logging.rs
- **Wave 2**: `feat(config): implement config system` — src/config/*.rs
- **Wave 2**: `feat(adapter): define Adapter trait with async-trait` — src/adapter/trait.rs, src/adapter/context.rs
- **Wave 2**: `feat(adapter): implement OnionExecutor with header passthrough` — src/adapter/chain.rs ⭐
- **Wave 2**: `feat(types): define GatewayResponse with axum Sse` — src/types/response.rs ⭐
- **Wave 2**: `feat(adapter): implement builtin adapters` — src/adapter/builtin/*.rs
- **Wave 2**: `feat(provider): implement HTTP client and registry` — src/provider/*.rs
- **Wave 2**: `feat(sse): implement SSE converter` — src/utils/sse.rs
- **Wave 3**: `feat(routes): implement OpenAI route (unified SSE)` — src/routes/openai.rs
- **Wave 3**: `feat(routes): implement Anthropic route (unified SSE)` — src/routes/anthropic.rs
- **Wave 3**: `feat(routes): add health and models endpoints` — src/routes/health.rs, src/routes/models.rs
- **Wave 3**: `feat(middleware): configure CORS and tracing` — src/routes/middleware.rs
- **Wave 3**: `feat(main): assemble application` — src/main.rs
- **Wave 4**: `test(unit): add unit tests` — src/**/*.rs
- **Wave 4**: `test(integration): add integration tests` — tests/integration/*.rs
- **Wave 4**: `test(e2e): perform manual testing` — tests/e2e/README.md
- **Wave 4**: `perf(bench): add benchmarks` — benches/*.rs

---

## Success Criteria

### 验证命令
```bash
cargo check                           # 预期：编译成功
cargo test                            # 预期：所有测试通过
cargo clippy                          # 预期：无 lint 警告
cargo run                             # 预期：服务器启动在 5564 端口

# 测试非流式请求
curl -X POST http://localhost:5564/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"model": "qwen-code@coder-model", "messages": [{"role": "user", "content": "Hello"}], "stream": false}'
# 预期：返回 OpenAI 格式 JSON 响应

# 测试流式请求
curl -N -X POST http://localhost:5564/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"model": "qwen-code@coder-model", "messages": [{"role": "user", "content": "Hello"}], "stream": true}'
# 预期：返回 SSE 流

# 测试响应头透传
curl -v -X POST http://localhost:5564/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"model": "qwen-code@coder-model", "messages": [{"role": "user", "content": "Hello"}]}'
# 预期：响应头包含 x-ratelimit-* 等上游 headers
```

### 最终检查清单
- [ ] 所有 "Must Have" 已实现
- [ ] 所有 "Must NOT Have" 不存在
- [ ] 所有测试通过
- [ ] 所有 QA 场景证据已捕获
- [ ] **响应头正确透传**
- [ ] Final Verification Wave 全部 APPROVE
