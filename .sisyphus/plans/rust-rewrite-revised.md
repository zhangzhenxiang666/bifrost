# Rust 重写 LLM Gateway - 修订版

> **修订日期**: 2026-03-01  
> **修订原因**: 用户反馈关键设计变更 - 移除 async-trait，结构化返回，统一 SSE 处理

## TL;DR

> **核心变更** (相比初版):
> 1. ❌ **移除 async-trait 依赖** - 使用 `Pin<Box<dyn Future>>` 手动实现异步
> 2. ✅ **结构化返回类型** - 适配器返回 `RequestTransform`/`ResponseTransform` struct
> 3. ✅ **统一路由处理 SSE** - 根据 `stream: bool` 返回 `GatewayResponse::Json` 或 `GatewayResponse::Sse`
> 4. ❌ **废除 method 字段** - 所有请求都是 POST
> 
> **关键设计**:
> - 适配器 trait: `fn transform_request(...) -> Pin<Box<dyn Future<Output = Result<Transform, Error>>>>`
> - `RequestTransform { body, url: Option, headers: Option }`
> - `GatewayResponse::Json(Json<Value>) | GatewayResponse::Sse(Sse<Stream>)`
> - SSE 转换在执行器统一处理，适配器只转换数据
> 
> **技术栈**: Axum 0.8.8 + Tokio 1.49.0 + reqwest 0.12 (无 async-trait)
> **任务数**: 24 任务 (新增 1 个，删除 1 个)
> **并行执行**: 4 个 Wave，最多 7 任务并行

---

## Core Design Changes

### 1. Adapter Trait (无 async-trait)

```rust
use std::future::Future;
use std::pin::Pin;

pub trait Adapter: Send + Sync {
    type Error: std::error::Error + Send + Sync;
    
    fn transform_request(
        &self,
        body: serde_json::Value,
        url: &str,
        headers: &http::HeaderMap,
    ) -> Pin<Box<dyn Future<Output = Result<RequestTransform, Self::Error>> + Send + '_>>;
    
    fn transform_response(
        &self,
        body: serde_json::Value,
        status: http::StatusCode,
        headers: &http::HeaderMap,
    ) -> Pin<Box<dyn Future<Output = Result<ResponseTransform, Self::Error>> + Send + '_>>;
    
    fn transform_stream_chunk(
        &self,
        chunk: serde_json::Value,
    ) -> Pin<Box<dyn Future<Output = Result<StreamChunkTransform, Self::Error>> + Send + '_>>;
}
```

### 2. 结构化返回类型

```rust
// 请求转换结果
pub struct RequestTransform {
    pub body: serde_json::Value,       // 必填 - 转换后的请求体
    pub url: Option<String>,            // 可选 - 如果要修改 URL
    pub headers: Option<http::HeaderMap>, // 可选 - 要添加的额外请求头
}

// 响应转换结果
pub struct ResponseTransform {
    pub body: serde_json::Value,
    pub status: Option<http::StatusCode>,
    pub headers: Option<http::HeaderMap>,
}

// 流式块转换结果
pub struct StreamChunkTransform {
    pub data: serde_json::Value,
    pub event: Option<String>,  // 可选 - SSE event 类型
}
```

### 3. 统一响应类型

```rust
// 类型别名简化 SSE 流类型
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

### 4. 路由处理 (统一 SSE)

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

### 用户反馈 (2026-03-01)

**核心操作点**: **请求头和请求体的转换** - 这是本项目的主要操作点

**关键设计变更**:
1. ❌ 移除 `async-trait` - 使用 `Pin<Box<dyn Future>>`
2. ✅ 结构化返回 - `RequestTransform`/`ResponseTransform`
3. ❌ 废除 `method` 字段 - 所有请求都是 POST
4. ✅ 统一路由 - 根据 `stream: bool` 返回不同类型
5. ✅ SSE 执行器统一处理 - 适配器只转换数据

### 技术决策

| 决策点 | 选择 | 理由 |
|--------|------|------|
| Future 返回 | `Pin<Box<dyn Future>>` | 成熟稳定，支持动态分发 |
| 错误类型 | 关联类型 `type Error` | 每个适配器自定义，链中统一转换 |
| HeaderMap | `http::HeaderMap` | axum 使用同一个 crate |
| SSE 处理 | 执行器统一转换 | 保持适配器职责单一 |

---

## Work Objectives

### 核心目标
实现与 Python 版本功能对等的 Rust LLM Gateway，利用 Rust 的类型安全和性能优势，采用手动 Future 实现避免 async-trait 依赖

### 具体交付物
- `src/config/` - 配置加载、解析、验证模块
- `src/adapter/` - 适配器 trait (无 async-trait)、执行器、内置适配器
- `src/provider/` - provider 管理、HTTP 客户端
- `src/routes/` - OpenAI 和 Anthropic 兼容路由 (统一 SSE 处理)
- `src/types/` - 共享类型定义（包括 Transform 类型）
- `src/error.rs` - 分层错误类型
- `tests/` - 单元和集成测试
- 更新 `Cargo.toml` (移除 async-trait)

### 完成定义
- [ ] 所有任务完成并通过测试
- [ ] 配置加载成功（使用 config.toml 验证）
- [ ] 请求转发到真实 LLM provider 成功
- [ ] 流式和非流式请求均正常工作（同一路由）
- [ ] 错误处理覆盖所有场景
- [ ] tracing 日志输出正常

### Must Have
- 配置系统支持 OneOrMany 反序列化
- 适配器链正确实现洋葱模型（使用 `Pin<Box<dyn Future>>`）
- 统一路由根据 `stream: bool` 返回正确类型
- 错误类型分层清晰
- 所有路由返回正确格式

### Must NOT Have (Guardrails)
- ❌ 不包含 async-trait 依赖
- ❌ 不包含自定义适配器动态加载
- ❌ 不包含 models[].adapter 字段
- ❌ 不使用运行时反射
- ❌ 不分离 SSE 路由（统一处理）

---

## Verification Strategy

### 测试决策
- **基础设施**: 使用 tokio-test + wiremock + mockall
- **自动化测试**: TDD 模式 - 每个任务先写测试再实现
- **框架**: cargo test (内置) + wiremock (HTTP mock)

### QA 策略
每个任务必须包含 Agent-Executed QA 场景：
- **API 测试**: 使用 curl 或 httpx 发送请求，验证响应
- **库/模块**: 使用 Rust 测试框架运行单元测试
- **集成测试**: 启动测试服务器，发送真实请求

证据保存到 `.sisyphus/evidence/task-{N}-{scenario-slug}.{ext}`

---

## Execution Strategy

### 并行执行 Waves

```
Wave 1 (基础架构 - 可立即开始):
├── Task 1: 项目脚手架 + Cargo.toml 依赖配置 [quick]
├── Task 2: 错误类型定义 (thiserror) [quick]
├── Task 3: 配置结构设计 (OneOrMany) [unspecified-high]
├── Task 4: 类型定义 (Newtype + Transform 类型) [quick]
├── Task 5: 配置加载和验证 [unspecified-high]
└── Task 6: tracing 日志配置 [quick]

Wave 2 (核心模块 - 依赖 Wave 1):
├── Task 7: Adapter trait 定义 (无 async-trait) [unspecified-high]
├── Task 8: 洋葱模型执行器 (OnionExecutor) [deep]
├── Task 9: GatewayResponse enum 定义 [quick]
├── Task 10: Passthrough 适配器实现 [quick]
├── Task 11: OpenAIToQwen 适配器实现 [unspecified-high]
├── Task 12: HTTP 客户端封装 (reqwest) [quick]
├── Task 13: Provider 注册表和管理 [unspecified-high]
└── Task 14: SSE 转换器实现 [quick]

Wave 3 (路由和集成 - 依赖 Wave 2):
├── Task 15: OpenAI 兼容路由 (统一 SSE) [deep]
├── Task 16: Anthropic 兼容路由 (统一 SSE) [deep]
├── Task 17: 健康检查和模型列表路由 [quick]
├── Task 18: CORS 和中间件配置 [quick]
└── Task 19: main.rs 应用组装 [quick]

Wave 4 (测试和验证 - 依赖 Wave 3):
├── Task 20: 单元测试编写 [deep]
├── Task 21: 集成测试编写 [deep]
├── Task 22: 端到端手动测试 [unspecified-high]
└── Task 23: 性能基准测试 [quick]

Wave FINAL (独立审查 - 4 个并行):
├── F1: 计划合规审计 (oracle)
├── F2: 代码质量审查 (unspecified-high)
├── F3: 真实手动 QA (unspecified-high)
└── F4: 范围保真检查 (deep)

关键路径: T1 → T3 → T5 → T7 → T8 → T15 → T21 → F1-F4
并行加速: ~65% 快于顺序执行
最大并发: 8 (Wave 2)
```

### 依赖矩阵

| 任务 | 依赖 | 被依赖 | Wave |
|------|------|--------|------|
| 1-6 | 无 | 7-14, 18, 19 | 1 |
| 7 | 2, 4, 6 | 8-11, 15-16 | 2 |
| 8 | 7 | 15-16, 20 | 2 |
| 9 | 2, 4 | 15-16, 19 | 2 |
| 10 | 7 | - | 2 |
| 11 | 7 | 15 | 2 |
| 12 | 2, 6 | 13, 15-16 | 2 |
| 13 | 3, 5, 12 | 15-16 | 2 |
| 14 | 4, 8 | 15-16 | 2 |
| 15 | 7-9, 11-14 | 19, 20-22 | 3 |
| 16 | 7-9, 12-14 | 19, 20-22 | 3 |
| 17 | 13 | 19 | 3 |
| 18 | 1 | 19 | 3 |
| 19 | 1, 3, 5, 15-18 | 20-23 | 3 |
| 20 | 7-17 | F1-F4 | 4 |
| 21 | 15-19 | F1-F4 | 4 |
| 22 | 15-19 | F1-F4 | 4 |
| 23 | 19 | F1-F4 | 4 |

### Agent 调度摘要

- **Wave 1**: 6 任务 - T1 → `quick`, T2 → `quick`, T3 → `unspecified-high`, T4 → `quick`, T5 → `unspecified-high`, T6 → `quick`
- **Wave 2**: 8 任务 - T7 → `unspecified-high`, T8 → `deep`, T9 → `quick`, T10 → `quick`, T11 → `unspecified-high`, T12 → `quick`, T13 → `unspecified-high`, T14 → `quick`
- **Wave 3**: 5 任务 - T15 → `deep`, T16 → `deep`, T17 → `quick`, T18 → `quick`, T19 → `quick`
- **Wave 4**: 4 任务 - T20 → `deep`, T21 → `deep`, T22 → `unspecified-high`, T23 → `quick`
- **FINAL**: 4 任务 - F1 → `oracle`, F2 → `unspecified-high`, F3 → `unspecified-high`, F4 → `deep`

---

## TODOs

> 注意：以下任务已根据用户反馈更新
> - Task 7: 移除 async-trait，使用 `Pin<Box<dyn Future>>`
> - Task 9: 新增 - GatewayResponse enum 定义
> - Task 14: 新增 - SSE 转换器实现
> - Task 15-16: 合并 SSE 和非 SSE 路由

- [ ] 1. 项目脚手架 + Cargo.toml 依赖配置

  **做什么**:
  - 更新 `Cargo.toml` 添加所有必需依赖
  - **不添加** `async-trait`
  - 创建基础目录结构
  - 创建 `src/lib.rs` 和 `src/main.rs`

  **必须不做**:
  - 不添加 async-trait 依赖
  - 不实现任何业务逻辑

  **推荐 Agent Profile**:
  - **Category**: `quick`
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 1
  - **阻塞**: T18, T19
  - **被阻塞**: 无

  **参考**:
  - Cargo.toml 依赖建议（移除 async-trait）

  **验收标准**:
  - [ ] `cargo check` 通过
  - [ ] 目录结构创建完成
  - [ ] Cargo.toml 不包含 async-trait

  **QA 场景**:
  ```
  场景：验证项目脚手架
    工具：Bash (cargo check)
    步骤:
      1. 运行 `cargo check`
      2. 检查无编译错误
      3. 检查 Cargo.toml 无 async-trait
    预期结果：编译成功，无 async-trait
    证据：.sisyphus/evidence/task-1-scaffold.txt
  ```

  **提交**: YES (与 T2, T4, T6 分组)
  - 消息：`chore(project): setup project scaffolding`
  - 文件：`Cargo.toml`, `src/lib.rs`, `src/main.rs`
  - 预提交：`cargo check`

---

- [ ] 2. 错误类型定义 (thiserror)

  **做什么**:
  - 创建 `src/error.rs`
  - 定义分层错误类型
  - 实现 `IntoResponse` trait

  **必须不做**:
  - 不使用 `anyhow::Error` 作为公共 API 错误

  **推荐 Agent Profile**:
  - **Category**: `quick`
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 1
  - **阻塞**: T7-T23
  - **被阻塞**: 无

  **参考**:
  - thiserror 官方文档

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

- [ ] 3. 配置结构设计 (OneOrMany)

  **做什么**:
  - 创建 `src/config/mod.rs`
  - 实现 `OneOrMany<T>` 结构
  - 定义配置结构体

  **必须不做**:
  - 不实现配置热重载

  **推荐 Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 1
  - **阻塞**: T5, T13, T19
  - **被阻塞**: 无

  **参考**:
  - Serde 自定义反序列化文档

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

- [ ] 4. 类型定义 (Newtype + Transform)

  **做什么**:
  - 创建 `src/types/mod.rs`
  - 定义 Newtype 类型：`ApiKey`, `ModelId`, `ProviderId`, `AdapterId`, `RequestId`
  - **新增**: 定义 `RequestTransform`, `ResponseTransform`, `StreamChunkTransform`

  **必须不做**:
  - 不在 `Display` 中暴露完整 API key

  **推荐 Agent Profile**:
  - **Category**: `quick`
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 1
  - **阻塞**: T7, T9, T14
  - **被阻塞**: 无

  **参考**:
  - Rust Newtype 模式

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

- [ ] 5. 配置加载和验证

  **做什么**:
  - 创建 `src/config/loader.rs` 和 `validator.rs`
  - 实现 `Config::from_file()` 和 `Config::validate()`

  **必须不做**:
  - 不实现配置文件监听

  **推荐 Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 1
  - **阻塞**: T13, T19
  - **被阻塞**: T3

  **参考**:
  - validator crate

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

- [ ] 6. tracing 日志配置

  **做什么**:
  - 创建 `src/utils/logging.rs`
  - 配置 `tracing-subscriber`
  - 实现 `init_logging()` 函数

  **必须不做**:
  - 不配置复杂的日志后端

  **推荐 Agent Profile**:
  - **Category**: `quick`
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 1
  - **阻塞**: 无
  - **被阻塞**: 无

  **参考**:
  - tracing crate

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

- [ ] 7. Adapter trait 定义 (无 async-trait)

  **做什么**:
  - 创建 `src/adapter/mod.rs`, `src/adapter/trait.rs`, `src/adapter/context.rs`
  - 定义 `Adapter` trait (使用 `Pin<Box<dyn Future>>`):
    ```rust
    fn transform_request(...) -> Pin<Box<dyn Future<Output = Result<...>> + Send + '_>>;
    ```
  - 定义 `RequestContext` 和 `ResponseContext`

  **必须不做**:
  - ❌ **不使用 async-trait**
  - 不使用 `Box<dyn Any>`

  **推荐 Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: NO
  - **并行组**: Wave 2 起始
  - **阻塞**: T8-T11, T15-T16
  - **被阻塞**: T2, T4, T6

  **参考**:
  - Rust Future trait 文档

  **验收标准**:
  - [ ] `Adapter` trait 定义完整（无 async-trait）
  - [ ] 使用 `Pin<Box<dyn Future>>` 返回
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
  - 消息：`feat(adapter): define Adapter trait without async-trait`
  - 文件：`src/adapter/trait.rs`, `src/adapter/context.rs`, `src/adapter/mod.rs`
  - 预提交：`cargo doc --no-deps`

---

- [ ] 8. 洋葱模型执行器 (OnionExecutor)

  **做什么**:
  - 创建 `src/adapter/chain.rs`
  - 实现 `OnionExecutor` 结构
  - 实现请求正向和响应反向执行

  **必须不做**:
  - 不实现实际的 HTTP 请求

  **推荐 Agent Profile**:
  - **Category**: `deep`
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: NO
  - **并行组**: Wave 2 关键路径
  - **阻塞**: T15-T16, T20-T22
  - **被阻塞**: T7

  **参考**:
  - Python executor.py

  **验收标准**:
  - [ ] 请求正向执行适配器链
  - [ ] 响应反向执行适配器链
  - [ ] 单元测试验证执行顺序

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
  ```

  **提交**: YES (单独提交)
  - 消息：`feat(adapter): implement OnionExecutor`
  - 文件：`src/adapter/chain.rs`
  - 预提交：`cargo test chain`

---

- [ ] 9. GatewayResponse enum 定义 (新增)

  **做什么**:
  - 创建 `src/types/response.rs` 或添加到 `src/types/mod.rs`
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

  **参考**:
  - axum IntoResponse 文档

  **验收标准**:
  - [ ] `GatewayResponse` enum 定义正确
  - [ ] 实现 `IntoResponse`
  - [ ] SSEStream 类型别名正确

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
  - 消息：`feat(types): define GatewayResponse enum for unified response`
  - 文件：`src/types/response.rs`
  - 预提交：`cargo test response`

---

- [ ] 10. Passthrough 适配器实现

  **做什么**:
  - 创建 `src/adapter/builtin/passthrough.rs`
  - 实现 `PassthroughAdapter`
  - 实现 `Adapter` trait（返回不修改的数据）

  **必须不做**:
  - 不添加复杂的转换逻辑

  **推荐 Agent Profile**:
  - **Category**: `quick`
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 2
  - **阻塞**: 无
  - **被阻塞**: T7

  **参考**:
  - Python adapters/passthrough.py

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

  **必须不做**:
  - 不硬编码 OAuth 凭证

  **推荐 Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 2
  - **阻塞**: T15
  - **被阻塞**: T7

  **参考**:
  - Python adapters/qwencode.py

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

  **必须不做**:
  - 不实现适配器逻辑

  **推荐 Agent Profile**:
  - **Category**: `quick`
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 2
  - **阻塞**: T13, T15-T16
  - **被阻塞**: T2, T6

  **参考**:
  - reqwest 文档

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

  **必须不做**:
  - 不实现适配器逻辑

  **推荐 Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 2
  - **阻塞**: T15-T16
  - **被阻塞**: T3, T5, T12

  **参考**:
  - Python core/config.py

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

- [ ] 14. SSE 转换器实现 (新增)

  **做什么**:
  - 创建 `src/utils/sse.rs`
  - 实现 `convert_to_sse(response: Value) -> SSEStream` 函数
  - 实现 `parse_sse_events(text: &str) -> Vec<Event>` 函数
  - 在执行器中调用此函数转换流式响应

  **必须不做**:
  - 不修改 SSE 数据内容（由适配器处理）

  **推荐 Agent Profile**:
  - **Category**: `quick`
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 2
  - **阻塞**: T15-T16
  - **被阻塞**: T4, T8

  **参考**:
  - axum SSE 文档

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
  - 消息：`feat(sse): implement SSE converter for streaming`
  - 文件：`src/utils/sse.rs`
  - 预提交：`cargo test sse`

---

- [ ] 15. OpenAI 兼容路由 (统一 SSE)

  **做什么**:
  - 创建 `src/routes/openai.rs`
  - 实现 `chat_completions` 处理函数:
    - 解析 `provider@model` 格式
    - 根据 `stream: bool` 返回 `GatewayResponse::Json` 或 `GatewayResponse::Sse`
  - **不创建**单独的路由处理 SSE

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

  **参考**:
  - Python routes/openai.py

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
  ```

  **提交**: YES (单独提交)
  - 消息：`feat(routes): implement OpenAI route with unified SSE handling`
  - 文件：`src/routes/openai.rs`, `src/routes/mod.rs`
  - 预提交：`cargo test routes`

---

- [ ] 16. Anthropic 兼容路由 (统一 SSE)

  **做什么**:
  - 创建 `src/routes/anthropic.rs`
  - 实现 `messages` 处理函数（类似 T15）
  - 根据 `stream: bool` 返回不同类型

  **必须不做**:
  - 不实现适配器逻辑
  - 不分离 SSE 路由

  **推荐 Agent Profile**:
  - **Category**: `deep`
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 3
  - **阻塞**: T19, T20-T22
  - **被阻塞**: T7-T9, T12-T14

  **参考**:
  - Python routes/anthropic.py

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
  - 消息：`feat(routes): implement Anthropic route with unified SSE handling`
  - 文件：`src/routes/anthropic.rs`
  - 预提交：`cargo test routes`

---

- [ ] 17. 健康检查和模型列表路由

  **做什么**:
  - 创建 `src/routes/health.rs`, `src/routes/models.rs`
  - 实现 `health_check` 和 `list_models`

  **必须不做**:
  - 不实现复杂的健康检查逻辑

  **推荐 Agent Profile**:
  - **Category**: `quick`
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 3
  - **阻塞**: T19
  - **被阻塞**: T13

  **参考**:
  - OpenAI /v1/models API

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

  **必须不做**:
  - 不配置过于严格的 CORS

  **推荐 Agent Profile**:
  - **Category**: `quick`
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 3
  - **阻塞**: T19
  - **被阻塞**: T1

  **参考**:
  - tower-http crate

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

  **必须不做**:
  - 不实现业务逻辑

  **推荐 Agent Profile**:
  - **Category**: `quick`
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: NO
  - **并行组**: Wave 3 收尾
  - **阻塞**: T20-T23
  - **被阻塞**: T1, T3, T5, T15-T18

  **参考**:
  - Python main.py

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

  **必须不做**:
  - 不写集成测试

  **推荐 Agent Profile**:
  - **Category**: `deep`
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: NO
  - **并行组**: Wave 4 起始
  - **阻塞**: F1-F4
  - **被阻塞**: T7-T18

  **参考**:
  - Rust 测试文档

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

  **必须不做**:
  - 不使用真实 API key

  **推荐 Agent Profile**:
  - **Category**: `deep`
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 4
  - **阻塞**: F1-F4
  - **被阻塞**: T15-T19

  **参考**:
  - wiremock crate

  **验收标准**:
  - [ ] 集成测试覆盖所有路由
  - [ ] 使用 mock 服务器
  - [ ] 测试流式和非流式

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
  - 记录所有测试结果

  **必须不做**:
  - 不使用 mock

  **推荐 Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 4
  - **阻塞**: F1-F4
  - **被阻塞**: T19

  **参考**:
  - Python 项目的使用方式

  **验收标准**:
  - [ ] 所有场景测试通过
  - [ ] 响应格式正确
  - [ ] 错误处理正确

  **QA 场景**:
  ```
  场景：端到端 OpenAI 流式测试
    工具：Bash (curl)
    步骤:
      1. curl -N -X POST http://localhost:5564/v1/chat/completions
      2. Body: {"model": "qwen-code@...", "stream": true}
      3. 断言收到 SSE 流
      4. 断言流以 [DONE] 结束
    预期结果：流式响应正确
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

  **必须不做**:
  - 不优化过早

  **推荐 Agent Profile**:
  - **Category**: `quick`
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: NO
  - **并行组**: Wave 4 收尾
  - **阻塞**: F1-F4
  - **被阻塞**: T19

  **参考**:
  - criterion crate

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
- **Wave 2**: `feat(adapter): implement Adapter trait (no async-trait)` — src/adapter/trait.rs, src/adapter/chain.rs
- **Wave 2**: `feat(types): define GatewayResponse enum` — src/types/response.rs
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
cargo check                           # 预期：编译成功，无错误
cargo test                            # 预期：所有测试通过
cargo clippy                          # 预期：无 lint 警告
cargo run                             # 预期：服务器启动在 5564 端口
curl http://localhost:5564/health     # 预期：200 OK
curl -X POST http://localhost:5564/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"model": "qwen-code@coder-model", "messages": [{"role": "user", "content": "Hello"}], "stream": false}'
                                      # 预期：返回 OpenAI 格式 JSON 响应
curl -N -X POST http://localhost:5564/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"model": "qwen-code@coder-model", "messages": [{"role": "user", "content": "Hello"}], "stream": true}'
                                      # 预期：返回 SSE 流
```

### 最终检查清单
- [ ] 所有 "Must Have" 已实现
- [ ] 所有 "Must NOT Have" 不存在
- [ ] 所有测试通过
- [ ] 所有 QA 场景证据已捕获
- [ ] Final Verification Wave 全部 APPROVE
