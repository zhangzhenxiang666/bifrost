# Rust 重写 LLM Gateway

## TL;DR

> **核心目标**: 将 Python FastAPI LLM Gateway 重写为 Rust Axum 实现，移除动态适配器支持，添加 provider/model 级别的 headers/body 配置
> 
> **关键变更**:
> - 移除 `custom-adapter` 配置（Rust 编译型语言不支持动态加载）
> - 移除 `models[].adapter` 字段（简化配置，同供应商接口一致）
> - 添加 `headers`/`body` 字段到 provider 和 model 配置
> 
> **技术栈**: Axum 0.8.8 + Tokio 1.49.0 + reqwest 0.12 + tracing + thiserror/anyhow
> 
> **交付物**:
> - 完整的 Rust LLM Gateway 实现
> - 配置系统（TOML 解析 + OneOrMany 支持）
> - 适配器链系统（洋葱模型：请求正向，响应反向）
> - OpenAI 和 Anthropic 兼容路由
> - 流式（SSE）和非流式请求支持
> - 完整的错误处理和日志追踪
> 
> **预计工作量**: 大型项目（20+ 任务）
> **并行执行**: 是 - 4 个 Wave，最多 7 个任务并行
> **关键路径**: 配置系统 → 适配器 Trait → 执行器 → 路由 → 集成测试

---

## Context

### 原始请求
使用 Rust 重写 Python LLM Gateway 项目，主要变更：
1. 移除自定义适配器支持（Rust 编译型语言限制）
2. 移除 models 字段的 adapter 支持（简化配置）
3. 为 provider 和 models 添加 headers/body 字段
4. 使用 Axum + Tokio 作为技术栈

### 原 Python 项目分析
- **Web 框架**: FastAPI
- **核心架构**: 适配器链（洋葱模型）
- **配置系统**: Pydantic + TOML
- **适配器**: 动态加载 Python 文件 + 内置适配器
- **执行器**: 请求正向执行适配器链，响应反向执行
- **路由**: `/openai/chat/completions` 和 `/anthropic/messages`

### Metis 差距分析关键发现
1. **架构差距**: Python 动态反射 vs Rust 静态 trait + 枚举分发
2. **配置解析**: 需要自定义 `OneOrMany<T>` deserializer 支持字符串/数组
3. **适配器链**: 使用 `async_trait` + `Next<'_>` 实现洋葱模型
4. **流式处理**: reqwest::stream → axum::Sse 转换
5. **错误处理**: 分层错误类型（ConfigError, AdapterError, GatewayError）
6. **类型安全**: Newtype 模式 + validator 库
7. **测试策略**: 单元测试 + 集成测试 + wiremock

---

## Work Objectives

### 核心目标
实现与 Python 版本功能对等的 Rust LLM Gateway，利用 Rust 的类型安全和性能优势

### 具体交付物
- `src/config/` - 配置加载、解析、验证模块
- `src/adapter/` - 适配器 trait、执行器、内置适配器
- `src/provider/` - provider 管理、HTTP 客户端
- `src/routes/` - OpenAI 和 Anthropic 兼容路由
- `src/types/` - 共享类型定义（Newtype 模式）
- `src/error.rs` - 分层错误类型
- `tests/` - 单元和集成测试
- 更新 `Cargo.toml` 添加所有依赖

### 完成定义
- [ ] 所有任务完成并通过测试
- [ ] 配置加载成功（使用 config.toml 验证）
- [ ] 请求转发到真实 LLM provider 成功
- [ ] 流式和非流式请求均正常工作
- [ ] 错误处理覆盖所有场景
- [ ] tracing 日志输出正常

### Must Have
- 配置系统支持 OneOrMany 反序列化
- 适配器链正确实现洋葱模型
- 流式 SSE 转发正常工作
- 错误类型分层清晰
- 所有路由返回正确格式

### Must NOT Have (Guardrails)
- 不包含自定义适配器动态加载（已明确移除）
- 不包含 models[].adapter 字段（已明确移除）
- 不使用运行时反射（如 `Any` trait）
- 不忽略错误（所有 ? 操作必须传播或显式处理）
- 不使用 `unwrap()` 生产代码（使用 ? 或 match）

---

## Verification Strategy

### 测试决策
- **基础设施**: 使用 tokio-test + wiremock + mockall
- **自动化测试**: TDD 模式 - 每个任务先写测试再实现
- **框架**: cargo test (内置) + wiremock (HTTP mock)

### QA 策略
每个任务必须包含 Agent-Executed QA 场景：
- **前端/UI**: 不适用（本项目为纯后端 API）
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
├── Task 4: 类型定义 (Newtype 模式) [quick]
├── Task 5: 配置加载和验证 [unspecified-high]
└── Task 6: tracing 日志配置 [quick]

Wave 2 (核心模块 - 依赖 Wave 1):
├── Task 7: Adapter trait 定义 + AdapterContext [unspecified-high]
├── Task 8: 洋葱模型执行器 (OnionExecutor) [deep]
├── Task 9: Passthrough 适配器实现 [quick]
├── Task 10: OpenAIToQwen 适配器实现 [unspecified-high]
├── Task 11: HTTP 客户端封装 (reqwest) [quick]
├── Task 12: Provider 注册表和管理 [unspecified-high]
└── Task 13: 请求/响应类型定义 [quick]

Wave 3 (路由和集成 - 依赖 Wave 2):
├── Task 14: OpenAI 兼容路由 (/v1/chat/completions) [deep]
├── Task 15: Anthropic 兼容路由 (/v1/messages) [deep]
├── Task 16: 流式 SSE 处理 [unspecified-high]
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

关键路径: T1 → T3 → T5 → T7 → T8 → T14 → T16 → T21 → F1-F4
并行加速: ~65% 快于顺序执行
最大并发: 7 (Wave 2)
```

### 依赖矩阵

| 任务 | 依赖 | 被依赖 | Wave |
|------|------|--------|------|
| 1-6 | 无 | 7-13, 19 | 1 |
| 7 | 2, 4, 6 | 8-10, 14-16 | 2 |
| 8 | 7 | 14-16, 20 | 2 |
| 9 | 7 | - | 2 |
| 10 | 7 | - | 2 |
| 11 | 2, 6 | 12, 14-16 | 2 |
| 12 | 3, 5, 11 | 14-15 | 2 |
| 13 | 2, 4 | 14-16 | 2 |
| 14 | 7, 8, 11-13 | 20-22 | 3 |
| 15 | 7, 8, 11-13 | 20-22 | 3 |
| 16 | 8, 11, 13 | 20-22 | 3 |
| 17 | 12 | - | 3 |
| 18 | 1 | 19 | 3 |
| 19 | 1, 3, 5, 14-18 | 20-23 | 3 |
| 20 | 7-16 | F1-F4 | 4 |
| 21 | 14-19 | F1-F4 | 4 |
| 22 | 14-19 | F1-F4 | 4 |
| 23 | 19 | F1-F4 | 4 |

### Agent 调度摘要

- **Wave 1**: 6 任务 - T1 → `quick`, T2 → `quick`, T3 → `unspecified-high`, T4 → `quick`, T5 → `unspecified-high`, T6 → `quick`
- **Wave 2**: 7 任务 - T7 → `unspecified-high`, T8 → `deep`, T9 → `quick`, T10 → `unspecified-high`, T11 → `quick`, T12 → `unspecified-high`, T13 → `quick`
- **Wave 3**: 6 任务 - T14 → `deep`, T15 → `deep`, T16 → `unspecified-high`, T17 → `quick`, T18 → `quick`, T19 → `quick`
- **Wave 4**: 4 任务 - T20 → `deep`, T21 → `deep`, T22 → `unspecified-high`, T23 → `quick`
- **FINAL**: 4 任务 - F1 → `oracle`, F2 → `unspecified-high`, F3 → `unspecified-high`, F4 → `deep`

---

## TODOs

- [ ] 1. 项目脚手架 + Cargo.toml 依赖配置

  **做什么**:
  - 更新 `Cargo.toml` 添加所有必需依赖
  - 创建基础目录结构：`src/config/`, `src/adapter/`, `src/provider/`, `src/routes/`, `src/types/`, `tests/`
  - 创建 `src/lib.rs` 作为库入口
  - 更新 `src/main.rs` 作为二进制入口

  **必须不做**:
  - 不实现任何业务逻辑
  - 不添加未计划的功能

  **推荐 Agent Profile**:
  - **Category**: `quick`
    - 原因：纯配置文件和目录结构创建，无复杂逻辑
  - **Skills**: `[]`
    - 不需要特殊技能

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 1 (与 T2, T4, T6)
  - **阻塞**: T18, T19
  - **被阻塞**: 无

  **参考**:
  - Metis 分析报告中的 "Cargo.toml 依赖建议" 部分
  - 当前 `Cargo.toml` 内容

  **验收标准**:
  - [ ] `cargo check` 通过（依赖下载完成）
  - [ ] 目录结构创建完成
  - [ ] `src/lib.rs` 和 `src/main.rs` 存在

  **QA 场景**:

  ```
  场景：验证项目脚手架
    工具：Bash (cargo)
    前置条件：在 /home/zzx/Codespace/rust_code/llm-map 目录
    步骤:
      1. 运行 `cargo check`
      2. 检查输出是否包含 "Finished" 且无错误
      3. 运行 `ls -la src/` 验证目录结构
    预期结果：cargo check 成功，所有必需目录存在
    失败指标：编译错误或目录缺失
    证据：.sisyphus/evidence/task-1-scaffold.txt
  ```

  **证据捕获**:
  - [ ] `cargo check` 输出保存到证据文件
  - [ ] 目录树截图或输出

  **提交**: YES (与 T2, T4, T6 分组)
  - 消息：`chore(project): setup project scaffolding and dependencies`
  - 文件：`Cargo.toml`, `src/lib.rs`, `src/main.rs`
  - 预提交：`cargo check`

---

- [ ] 2. 错误类型定义 (thiserror)

  **做什么**:
  - 创建 `src/error.rs`
  - 定义分层错误类型：
    - `GatewayError` - 顶层错误
    - `ConfigError` - 配置相关错误
    - `AdapterError` - 适配器相关错误
    - `ProviderError` - provider 相关错误
  - 实现 `IntoResponse` trait 用于 axum 响应
  - 实现 `From` trait 用于错误转换

  **必须不做**:
  - 不在错误类型中包含敏感信息（如完整 API key）
  - 不使用 `anyhow::Error` 作为公共 API 错误

  **推荐 Agent Profile**:
  - **Category**: `quick`
    - 原因：纯类型定义，无复杂业务逻辑
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 1 (与 T1, T4, T6)
  - **阻塞**: T7-T23（所有后续任务）
  - **被阻塞**: 无

  **参考**:
  - Metis 报告 "错误处理策略" 部分
  - thiserror 官方文档：https://docs.rs/thiserror/latest/thiserror/

  **验收标准**:
  - [ ] `src/error.rs` 包含所有错误类型定义
  - [ ] 实现 `IntoResponse` for axum 集成
  - [ ] 实现 `From` trait 用于自动转换
  - [ ] `cargo test error` 通过

  **QA 场景**:

  ```
  场景：验证错误类型转换
    工具：Bash (cargo test)
    前置条件：错误类型已定义
    步骤:
      1. 创建测试文件 tests/error_tests.rs
      2. 编写测试：ConfigError → GatewayError 自动转换
      3. 编写测试：AdapterError → GatewayError 自动转换
      4. 运行 `cargo test error`
    预期结果：所有错误转换测试通过
    证据：.sisyphus/evidence/task-2-error-tests.txt
  ```

  **证据捕获**:
  - [ ] 测试输出保存

  **提交**: YES (与 T1, T4, T6 分组)
  - 消息：`feat(error): define layered error types with thiserror`
  - 文件：`src/error.rs`
  - 预提交：`cargo test error`

---

- [ ] 3. 配置结构设计 (OneOrMany)

  **做什么**:
  - 创建 `src/config/mod.rs`
  - 实现 `OneOrMany<T>` 结构支持字符串/数组反序列化
  - 定义 `ProviderConfig`, `ModelConfig`, `HeaderField`, `BodyField` 结构
  - 实现自定义 `Deserialize` trait for `OneOrMany`
  - 添加 `#[serde]` 属性支持灵活 TOML 格式

  **必须不做**:
  - 不实现配置热重载（已明确不需要）
  - 不使用 `HashMap` 存储 provider（使用有序 `BTreeMap` 保持一致性）

  **推荐 Agent Profile**:
  - **Category**: `unspecified-high`
    - 原因：需要理解 Serde 反序列化机制和自定义 visitor 模式
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 1 (与 T1, T2, T4, T6)
  - **阻塞**: T5, T12, T19
  - **被阻塞**: 无

  **参考**:
  - Metis 报告 "配置解析：OneOrMany 实现" 部分
  - Serde 自定义反序列化文档：https://serde.rs/custom-deserialization.html

  **验收标准**:
  - [ ] `OneOrMany<String>` 可反序列化 `"adapter"` 和 `["a", "b"]`
  - [ ] `ProviderConfig` 包含所有必需字段
  - [ ] `ModelConfig` 不包含 `adapter` 字段（已移除）
  - [ ] `BodyField` 支持多种类型（string/number/boolean/json）

  **QA 场景**:

  ```
  场景：验证 OneOrMany 反序列化
    工具：Bash (cargo test)
    前置条件：OneOrMany 结构已定义
    步骤:
      1. 创建测试配置字符串：adapter = "single"
      2. 创建测试配置字符串：adapter = ["a", "b", "c"]
      3. 使用 toml::from_str 解析
      4. 断言结果都是 OneOrMany(vec![...])
    预期结果：两种格式都正确解析为 Vec
    证据：.sisyphus/evidence/task-3-one-or-many-test.txt
  ```

  **证据捕获**:
  - [ ] 测试代码和输出

  **提交**: YES (单独提交)
  - 消息：`feat(config): implement OneOrMany deserializer for flexible TOML`
  - 文件：`src/config/mod.rs`
  - 预提交：`cargo test config`

---

- [ ] 4. 类型定义 (Newtype 模式)

  **做什么**:
  - 创建 `src/types/mod.rs`
  - 定义 Newtype 类型：
    - `ApiKey(String)` - 带 mask 显示的 API key
    - `ModelId(String)` - 模型标识符
    - `ProviderId(String)` - provider 标识符
    - `AdapterId(String)` - 适配器标识符
    - `RequestId(Uuid)` - 请求 ID（用于追踪）
  - 实现 `Display`, `Clone`, `Serialize`, `Deserialize` trait

  **必须不做**:
  - 不在 `Display` 实现中暴露完整 API key

  **推荐 Agent Profile**:
  - **Category**: `quick`
    - 原因：简单的类型定义和 trait 实现
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 1 (与 T1, T2, T6)
  - **阻塞**: T7, T13
  - **被阻塞**: 无

  **参考**:
  - Metis 报告 "类型安全：配置与请求验证" 部分
  - Rust Newtype 模式：https://doc.rust-lang.org/book/ch19-04-advanced-types.html

  **验收标准**:
  - [ ] 所有 Newtype 类型定义完成
  - [ ] `ApiKey.mask()` 方法正确隐藏敏感信息
  - [ ] 所有类型实现必要的 trait

  **QA 场景**:

  ```
  场景：验证 ApiKey 安全性
    工具：Bash (cargo test)
    前置条件：ApiKey 类型已定义
    步骤:
      1. 创建 ApiKey("sk-very-long-secret-key-12345")
      2. 调用 .mask() 方法
      3. 断言结果不包含完整 key
      4. 调用 .to_string() 验证也不暴露
    预期结果：mask 返回 "sk-ve****2345" 格式
    证据：.sisyphus/evidence/task-4-apikey-mask.txt
  ```

  **证据捕获**:
  - [ ] 测试输出

  **提交**: YES (与 T1, T2, T6 分组)
  - 消息：`feat(types): define Newtype wrappers for type safety`
  - 文件：`src/types/mod.rs`
  - 预提交：`cargo test types`

---

- [ ] 5. 配置加载和验证

  **做什么**:
  - 在 `src/config/` 创建 `loader.rs` 和 `validator.rs`
  - 实现 `Config::from_file(path: &str)` 方法
  - 实现 `Config::validate()` 方法检查:
    - 所有 adapter 名称存在于内置适配器列表
    - base_url 格式正确
    - 必需字段不为空
  - 使用 `validator` crate 进行派生验证

  **必须不做**:
  - 不实现配置文件监听（不需要热重载）

  **推荐 Agent Profile**:
  - **Category**: `unspecified-high`
    - 原因：需要理解 validator crate 和 TOML 解析
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 1 (与 T3)
  - **阻塞**: T12, T19
  - **被阻塞**: T3 (配置结构)

  **参考**:
  - Metis 报告配置验证部分
  - validator crate: https://docs.rs/validator/latest/validator/

  **验收标准**:
  - [ ] `Config::from_file()` 正确解析 config.toml
  - [ ] `Config::validate()` 检查所有 adapter 存在
  - [ ] 无效配置返回清晰的错误信息

  **QA 场景**:

  ```
  场景：验证配置加载
    工具：Bash (cargo test)
    前置条件：config.toml 存在
    步骤:
      1. 创建测试配置文件 tests/fixtures/config/valid.toml
      2. 创建测试配置文件 tests/fixtures/config/invalid-adapter.toml
      3. 调用 Config::from_file() 加载
      4. 调用 Config::validate() 验证
    预期结果：有效配置通过，无效配置返回错误
    证据：.sisyphus/evidence/task-5-config-load.txt
  ```

  **证据捕获**:
  - [ ] 测试配置文件
  - [ ] 测试输出

  **提交**: YES (单独提交)
  - 消息：`feat(config): implement config loader and validator`
  - 文件：`src/config/loader.rs`, `src/config/validator.rs`
  - 预提交：`cargo test config`

---

- [ ] 6. tracing 日志配置

  **做什么**:
  - 创建 `src/utils/logging.rs`
  - 配置 `tracing-subscriber` 支持:
    - 环境变量控制日志级别（RUST_LOG）
    - JSON 格式输出（可选）
    - 时间戳和模块路径
  - 实现 `init_logging()` 函数在 main.rs 调用
  - 在关键位置添加 `tracing::info!`, `tracing::error!` 宏

  **必须不做**:
  - 不配置复杂的日志后端（如 ELK stack）

  **推荐 Agent Profile**:
  - **Category**: `quick`
    - 原因：标准配置，无复杂逻辑
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 1 (与 T1, T2, T4)
  - **阻塞**: 无（所有任务都可使用 tracing）
  - **被阻塞**: 无

  **参考**:
  - tracing crate: https://docs.rs/tracing/latest/tracing/
  - tracing-subscriber: https://docs.rs/tracing-subscriber/latest/tracing_subscriber/

  **验收标准**:
  - [ ] `init_logging()` 函数正确配置 subscriber
  - [ ] `RUST_LOG=debug` 可控制日志级别
  - [ ] 日志包含时间戳和模块路径

  **QA 场景**:

  ```
  场景：验证日志输出
    工具：Bash (cargo run)
    前置条件：main.rs 调用 init_logging()
    步骤:
      1. 设置 RUST_LOG=debug
      2. 运行 cargo run
      3. 检查输出包含日志信息
    预期结果：看到带时间戳和模块的日志
    证据：.sisyphus/evidence/task-6-logging.txt
  ```

  **证据捕获**:
  - [ ] 日志输出截图

  **提交**: YES (与 T1, T2, T4 分组)
  - 消息：`feat(logging): configure tracing with tracing-subscriber`
  - 文件：`src/utils/logging.rs`, `src/utils/mod.rs`
  - 预提交：`RUST_LOG=debug cargo run`

---

- [ ] 7. Adapter trait 定义 + AdapterContext

  **做什么**:
  - 创建 `src/adapter/mod.rs`, `src/adapter/trait.rs`, `src/adapter/context.rs`
  - 定义 `Adapter` trait (使用 `#[async_trait]`):
    - `async fn process_request(&self, ctx: &mut RequestContext, next: Next<'_>) -> Result<ResponseContext>`
    - `async fn process_response(&self, ctx: &mut ResponseContext) -> Result<()>`
  - 定义 `RequestContext` 包含：body, headers, extra
  - 定义 `ResponseContext` 包含：status, headers, body
  - 定义 `Next<'_>` 结构用于链式调用

  **必须不做**:
  - 不实现具体适配器（后续任务）
  - 不使用 `Box<dyn Any>` 等运行时类型

  **推荐 Agent Profile**:
  - **Category**: `unspecified-high`
    - 原因：核心架构设计，需要理解洋葱模型和 Rust 生命周期
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: NO
  - **并行组**: Wave 2 起始任务
  - **阻塞**: T8-T10, T14-T16
  - **被阻塞**: T2, T4, T6

  **参考**:
  - Metis 报告 "适配器链：洋葱模型实现" 部分
  - async-trait crate: https://docs.rs/async-trait/latest/async_trait/

  **验收标准**:
  - [ ] `Adapter` trait 定义完整
  - [ ] `RequestContext` 和 `ResponseContext` 包含所有必需字段
  - [ ] `Next<'_>` 正确实现链式调用
  - [ ] `cargo doc` 生成文档无警告

  **QA 场景**:

  ```
  场景：验证 Adapter trait 编译
    工具：Bash (cargo check)
    前置条件：trait 定义完成
    步骤:
      1. 运行 cargo check
      2. 检查无编译错误
      3. 运行 cargo doc --no-deps
      4. 检查文档生成成功
    预期结果：编译和文档生成都成功
    证据：.sisyphus/evidence/task-7-adapter-trait.txt
  ```

  **证据捕获**:
  - [ ] 文档生成输出

  **提交**: YES (单独提交)
  - 消息：`feat(adapter): define Adapter trait and context types`
  - 文件：`src/adapter/trait.rs`, `src/adapter/context.rs`, `src/adapter/mod.rs`
  - 预提交：`cargo doc --no-deps`

---

- [ ] 8. 洋葱模型执行器 (OnionExecutor)

  **做什么**:
  - 创建 `src/adapter/chain.rs`
  - 实现 `OnionExecutor` 结构:
    - `new(adapters: Vec<Arc<dyn Adapter>>)` - 构造函数
    - `async execute(&self, ctx: RequestContext, terminal: TerminalHandler) -> Result<ResponseContext>` - 执行方法
  - 实现请求正向执行适配器链
  - 实现响应反向执行适配器链
  - 定义 `TerminalHandler` 类型别名（实际 HTTP 请求发送）

  **必须不做**:
  - 不实现实际的 HTTP 请求（这是 terminal handler 的职责）

  **推荐 Agent Profile**:
  - **Category**: `deep`
    - 原因：核心业务逻辑，需要精确实现双向执行
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: NO
  - **并行组**: Wave 2 关键路径
  - **阻塞**: T14-T16, T20-T22
  - **被阻塞**: T7

  **参考**:
  - Metis 报告 "洋葱模型实现" 部分
  - Python executor.py 作为行为参考

  **验收标准**:
  - [ ] 请求时适配器按顺序正向执行
  - [ ] 响应时适配器按顺序反向执行
  - [ ] 支持空适配器链（直接调用 terminal）
  - [ ] 单元测试验证执行顺序

  **QA 场景**:

  ```
  场景：验证洋葱模型执行顺序
    工具：Bash (cargo test)
    前置条件：OnionExecutor 已实现
    步骤:
      1. 创建 3 个 Mock 适配器，每个记录执行顺序
      2. 创建 mock terminal handler
      3. 执行请求
      4. 断言请求顺序：A → B → C → Terminal
      5. 断言响应顺序：Terminal → C → B → A
    预期结果：执行顺序符合洋葱模型
    证据：.sisyphus/evidence/task-8-onion-order.txt
  ```

  **证据捕获**:
  - [ ] 测试代码和输出

  **提交**: YES (单独提交)
  - 消息：`feat(adapter): implement OnionExecutor for chain execution`
  - 文件：`src/adapter/chain.rs`
  - 预提交：`cargo test chain`

---

- [ ] 9. Passthrough 适配器实现

  **做什么**:
  - 创建 `src/adapter/builtin/passthrough.rs`
  - 实现 `PassthroughAdapter` 结构:
    - 不修改请求 body
    - 不修改响应 body
    - 仅添加必要的 headers（如 Authorization）
  - 实现 `Adapter` trait

  **必须不做**:
  - 不添加复杂的转换逻辑

  **推荐 Agent Profile**:
  - **Category**: `quick`
    - 原因：最简单的适配器实现
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 2 (与 T11, T13)
  - **阻塞**: 无
  - **被阻塞**: T7

  **参考**:
  - Python adapters/passthrough.py
  - Metis 报告适配器实现示例

  **验收标准**:
  - [ ] 实现 Adapter trait
  - [ ] 请求和响应都不修改
  - [ ] 正确添加 Authorization header

  **QA 场景**:

  ```
  场景：验证 Passthrough 不修改数据
    工具：Bash (cargo test)
    前置条件：PassthroughAdapter 已实现
    步骤:
      1. 创建请求上下文包含特定 body
      2. 执行 process_request
      3. 断言 body 不变
      4. 执行 process_response
      5. 断言响应不变
    预期结果：数据完全不变
    证据：.sisyphus/evidence/task-9-passthrough.txt
  ```

  **证据捕获**:
  - [ ] 测试输出

  **提交**: YES (与 T11, T13 分组)
  - 消息：`feat(adapter): implement PassthroughAdapter`
  - 文件：`src/adapter/builtin/passthrough.rs`, `src/adapter/builtin/mod.rs`
  - 预提交：`cargo test passthrough`

---

- [ ] 10. OpenAIToQwen 适配器实现

  **做什么**:
  - 创建 `src/adapter/builtin/openai_to_qwen.rs`
  - 实现 `OpenAIToQwenAdapter` 结构:
    - 转换 OpenAI 格式 → Qwen 格式
    - 处理 OAuth token 刷新（如果需要）
    - 添加 Qwen 特定的 headers
  - 实现 `Adapter` trait

  **必须不做**:
  - 不硬编码 OAuth 凭证（从配置读取）

  **推荐 Agent Profile**:
  - **Category**: `unspecified-high`
    - 原因：需要理解 OpenAI 和 Qwen API 格式差异
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 2 (与 T12)
  - **阻塞**: T14
  - **被阻塞**: T7

  **参考**:
  - Python adapters/qwencode.py
  - Qwen API 文档

  **验收标准**:
  - [ ] 正确转换 OpenAI messages → Qwen messages
  - [ ] 添加必要的 Qwen headers
  - [ ] 支持 OAuth token 管理（如果需要）

  **QA 场景**:

  ```
  场景：验证 OpenAI 到 Qwen 格式转换
    工具：Bash (cargo test)
    前置条件：OpenAIToQwenAdapter 已实现
    步骤:
      1. 创建 OpenAI 格式请求 body
      2. 执行 process_request
      3. 断言转换为 Qwen 格式
      4. 验证 headers 包含 X-DashScope-*
    预期结果：格式正确转换，headers 正确添加
    证据：.sisyphus/evidence/task-10-qwen-transform.txt
  ```

  **证据捕获**:
  - [ ] 测试输入输出对比

  **提交**: YES (单独提交)
  - 消息：`feat(adapter): implement OpenAIToQwenAdapter`
  - 文件：`src/adapter/builtin/openai_to_qwen.rs`
  - 预提交：`cargo test openai_to_qwen`

---

- [ ] 11. HTTP 客户端封装 (reqwest)

  **做什么**:
  - 创建 `src/provider/client.rs`
  - 实现 `HttpClient` 结构:
    - 使用 `reqwest::Client` 作为内部客户端
    - 实现 `async fn send_request(&self, method, url, headers, body) -> Result<Response>`
    - 实现 `async fn send_stream(&self, method, url, headers, body) -> Result<impl Stream>`
  - 配置连接池、超时、重试策略

  **必须不做**:
  - 不实现适配器逻辑（仅 HTTP 发送）

  **推荐 Agent Profile**:
  - **Category**: `quick`
    - 原因：reqwest 封装，标准 HTTP 客户端
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 2 (与 T9, T13)
  - **阻塞**: T12, T14-T16
  - **被阻塞**: T2, T6

  **参考**:
  - reqwest 文档：https://docs.rs/reqwest/latest/reqwest/
  - Metis 报告 "流式处理" 部分

  **验收标准**:
  - [ ] 支持非流式 POST 请求
  - [ ] 支持流式请求返回 Stream
  - [ ] 配置超时 600 秒
  - [ ] 启用连接池

  **QA 场景**:

  ```
  场景：验证 HTTP 客户端发送请求
    工具：Bash (cargo test)
    前置条件：HttpClient 已实现
    步骤:
      1. 启动 wiremock 测试服务器
      2. 配置 mock 响应
      3. 调用 send_request
      4. 断言收到正确响应
    预期结果：请求成功发送并接收响应
    证据：.sisyphus/evidence/task-11-http-client.txt
  ```

  **证据捕获**:
  - [ ] mock 服务器日志

  **提交**: YES (与 T9, T13 分组)
  - 消息：`feat(provider): implement HttpClient wrapper with reqwest`
  - 文件：`src/provider/client.rs`
  - 预提交：`cargo test client`

---

- [ ] 12. Provider 注册表和管理

  **做什么**:
  - 创建 `src/provider/registry.rs`
  - 实现 `ProviderRegistry` 结构:
    - `new(config: &Config)` - 从配置初始化
    - `get(&self, provider_name: &str) -> Option<Provider>` - 获取 provider
    - `get_adapter_chain(&self, provider_name: &str, model: &str) -> Vec<Arc<dyn Adapter>>` - 获取适配器链
  - 实现 `Provider` 结构包含：base_url, api_key, adapter_chain, headers, body
  - 合并 provider 级和 model 级的 headers/body

  **必须不做**:
  - 不实现适配器逻辑（仅管理）

  **推荐 Agent Profile**:
  - **Category**: `unspecified-high`
    - 原因：需要理解配置到运行时的映射
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 2 (与 T10)
  - **阻塞**: T14, T15
  - **被阻塞**: T3, T5, T11

  **参考**:
  - Python core/config.py get_provider_config
  - Metis 报告配置结构

  **验收标准**:
  - [ ] 从配置正确构建 provider
  - [ ] 正确合并 provider 和 model 的 headers/body
  - [ ] 构建适配器链

  **QA 场景**:

  ```
  场景：验证 Provider 注册表
    工具：Bash (cargo test)
    前置条件：ProviderRegistry 已实现
    步骤:
      1. 加载测试配置（包含多个 provider）
      2. 调用 get("qwen-code")
      3. 断言返回正确的 base_url, api_key
      4. 调用 get_adapter_chain
      5. 断言适配器链正确构建
    预期结果：provider 信息正确，适配器链完整
    证据：.sisyphus/evidence/task-12-registry.txt
  ```

  **证据捕获**:
  - [ ] 测试输出

  **提交**: YES (单独提交)
  - 消息：`feat(provider): implement ProviderRegistry for provider management`
  - 文件：`src/provider/registry.rs`, `src/provider/mod.rs`
  - 预提交：`cargo test registry`

---

- [ ] 13. 请求/响应类型定义

  **做什么**:
  - 创建 `src/types/request.rs`, `src/types/response.rs`
  - 定义 `ChatCompletionRequest` 结构（OpenAI 兼容）
  - 定义 `ChatCompletionResponse` 结构
  - 定义 `Message` 结构（role, content）
  - 定义 `StreamChunk` 结构（SSE 数据）
  - 实现 Serialize/Deserialize

  **必须不做**:
  - 不实现业务逻辑（仅类型定义）

  **推荐 Agent Profile**:
  - **Category**: `quick`
    - 原因：纯类型定义
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 2 (与 T9, T11)
  - **阻塞**: T14-T16
  - **被阻塞**: T4

  **参考**:
  - OpenAI API 文档
  - Python routes/openai.py

  **验收标准**:
  - [ ] 所有类型正确定义
  - [ ] 支持序列化和反序列化
  - [ ] 与 OpenAI API 兼容

  **QA 场景**:

  ```
  场景：验证请求类型序列化
    工具：Bash (cargo test)
    前置条件：类型定义完成
    步骤:
      1. 创建 ChatCompletionRequest 实例
      2. 序列化为 JSON
      3. 断言 JSON 格式符合 OpenAI 规范
      4. 反序列化回结构体
      5. 断言数据一致
    预期结果：序列化和反序列化都正确
    证据：.sisyphus/evidence/task-13-types.txt
  ```

  **证据捕获**:
  - [ ] JSON 示例

  **提交**: YES (与 T9, T11 分组)
  - 消息：`feat(types): define request/response types for OpenAI compatibility`
  - 文件：`src/types/request.rs`, `src/types/response.rs`
  - 预提交：`cargo test types`

---

- [ ] 14. OpenAI 兼容路由 (/v1/chat/completions)

  **做什么**:
  - 创建 `src/routes/openai.rs`
  - 实现 `chat_completions` 处理函数:
    - 解析 `provider@model` 格式
    - 从 ProviderRegistry 获取 provider
    - 构建 RequestContext
    - 调用 OnionExecutor 执行
    - 返回 JSONResponse 或 SSE
  - 添加路由到 axum router

  **必须不做**:
  - 不实现适配器逻辑（路由只负责协调）

  **推荐 Agent Profile**:
  - **Category**: `deep`
    - 原因：核心路由逻辑，需要协调多个组件
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: NO
  - **并行组**: Wave 3 关键路径
  - **阻塞**: T19, T20-T22
  - **被阻塞**: T7, T8, T10, T12, T13

  **参考**:
  - Python routes/openai.py
  - axum 路由文档

  **验收标准**:
  - [ ] 正确解析 provider@model 格式
  - [ ] 非流式请求返回 JSONResponse
  - [ ] 流式请求返回 SSE
  - [ ] 错误处理返回正确状态码

  **QA 场景**:

  ```
  场景：验证 OpenAI 路由非流式请求
    工具：Bash (curl)
    前置条件：路由已实现，服务器运行在 5564 端口
    步骤:
      1. 发送 POST /v1/chat/completions
      2. Body: {"model": "qwen-code@coder-model", "messages": [...], "stream": false}
      3. 断言响应状态 200
      4. 断言响应包含 choices 数组
    预期结果：返回符合 OpenAI 格式的响应
    证据：.sisyphus/evidence/task-14-openai-route.txt
  ```

  **证据捕获**:
  - [ ] curl 请求和响应

  **提交**: YES (单独提交)
  - 消息：`feat(routes): implement OpenAI-compatible /v1/chat/completions route`
  - 文件：`src/routes/openai.rs`, `src/routes/mod.rs`
  - 预提交：`cargo test routes`

---

- [ ] 15. Anthropic 兼容路由 (/v1/messages)

  **做什么**:
  - 创建 `src/routes/anthropic.rs`
  - 实现 `messages` 处理函数:
    - 解析 `provider@model` 格式
    - 从 ProviderRegistry 获取 provider
    - 构建 RequestContext（Anthropic 格式）
    - 调用 OnionExecutor 执行
    - 返回 JSONResponse 或 SSE
  - 添加路由到 axum router

  **必须不做**:
  - 不实现适配器逻辑

  **推荐 Agent Profile**:
  - **Category**: `deep`
    - 原因：核心路由逻辑，Anthropic 格式处理
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 3 (与 T16)
  - **阻塞**: T19, T20-T22
  - **被阻塞**: T7, T8, T12, T13

  **参考**:
  - Python routes/anthropic.py
  - Anthropic API 文档

  **验收标准**:
  - [ ] 正确解析 provider@model 格式
  - [ ] 非流式请求返回 JSONResponse
  - [ ] 流式请求返回 SSE
  - [ ] 错误处理返回正确状态码

  **QA 场景**:

  ```
  场景：验证 Anthropic 路由非流式请求
    工具：Bash (curl)
    前置条件：路由已实现，服务器运行在 5564 端口
    步骤:
      1. 发送 POST /v1/messages
      2. Body: {"model": "qwen-code@coder-model", "messages": [...]}
      3. 断言响应状态 200
      4. 断言响应符合 Anthropic 格式
    预期结果：返回符合 Anthropic 格式的响应
    证据：.sisyphus/evidence/task-15-anthropic-route.txt
  ```

  **证据捕获**:
  - [ ] curl 请求和响应

  **提交**: YES (单独提交)
  - 消息：`feat(routes): implement Anthropic-compatible /v1/messages route`
  - 文件：`src/routes/anthropic.rs`
  - 预提交：`cargo test routes`

---

- [ ] 16. 流式 SSE 处理

  **做什么**:
  - 在 `src/provider/client.rs` 或新建 `src/utils/sse.rs`
  - 实现 SSE 流转换:
    - 从 reqwest 获取字节流
    - 解析 SSE 事件（data: 前缀，空行分隔）
    - 转换为 axum::Sse 格式
  - 实现 `parse_sse_events(text: &str) -> Vec<Event>` 函数
  - 在路由中根据 `stream: true` 选择 SSE 响应

  **必须不做**:
  - 不修改 SSE 数据内容（由适配器处理）

  **推荐 Agent Profile**:
  - **Category**: `unspecified-high`
    - 原因：需要理解 SSE 协议和流式处理
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 3 (与 T15)
  - **阻塞**: T19, T20-T22
  - **被阻塞**: T8, T11, T13

  **参考**:
  - Metis 报告 "流式处理" 部分
  - axum SSE 文档：https://docs.rs/axum/latest/axum/response/sse/index.html

  **验收标准**:
  - [ ] 正确解析 SSE 事件
  - [ ] 正确处理多行 data 字段
  - [ ] 正确处理 [DONE] 标记
  - [ ] 保持连接活跃（keep-alive）

  **QA 场景**:

  ```
  场景：验证 SSE 流式转发
    工具：Bash (curl)
    前置条件：SSE 处理已实现，mock 服务器运行
    步骤:
      1. 启动 mock 服务器返回 SSE 流
      2. 发送流式请求到网关
      3. 使用 curl -N 接收流
      4. 断言收到正确的 SSE 事件
    预期结果：SSE 事件正确转发，格式保持
    证据：.sisyphus/evidence/task-16-sse-stream.txt
  ```

  **证据捕获**:
  - [ ] SSE 流输出

  **提交**: YES (单独提交)
  - 消息：`feat(sse): implement SSE streaming support with axum::Sse`
  - 文件：`src/utils/sse.rs`
  - 预提交：`cargo test sse`

---

- [ ] 17. 健康检查和模型列表路由

  **做什么**:
  - 创建 `src/routes/health.rs`, `src/routes/models.rs`
  - 实现 `health_check` 处理函数返回 200 OK
  - 实现 `list_models` 处理函数返回可用模型列表
  - 添加路由到 axum router

  **必须不做**:
  - 不实现复杂的健康检查逻辑

  **推荐 Agent Profile**:
  - **Category**: `quick`
    - 原因：简单的路由实现
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 3 (与 T18)
  - **阻塞**: T19
  - **被阻塞**: T12

  **参考**:
  - OpenAI /v1/models API

  **验收标准**:
  - [ ] GET /health 返回 200
  - [ ] GET /v1/models 返回模型列表

  **QA 场景**:

  ```
  场景：验证健康检查路由
    工具：Bash (curl)
    前置条件：路由已实现，服务器运行
    步骤:
      1. 发送 GET /health
      2. 断言状态码 200
      3. 断言响应包含 "ok" 或类似
    预期结果：健康检查通过
    证据：.sisyphus/evidence/task-17-health.txt
  ```

  **证据捕获**:
  - [ ] curl 输出

  **提交**: YES (与 T18 分组)
  - 消息：`feat(routes): add health check and models list endpoints`
  - 文件：`src/routes/health.rs`, `src/routes/models.rs`
  - 预提交：`curl http://localhost:5564/health`

---

- [ ] 18. CORS 和中间件配置

  **做什么**:
  - 创建 `src/routes/middleware.rs`
  - 配置 CORS 中间件（允许所有来源或配置来源）
  - 配置 tracing 中间件（记录请求日志）
  - 配置超时中间件
  - 添加到 axum router

  **必须不做**:
  - 不配置过于严格的 CORS（开发阶段）

  **推荐 Agent Profile**:
  - **Category**: `quick`
    - 原因：标准中间件配置
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 3 (与 T17)
  - **阻塞**: T19
  - **被阻塞**: T1

  **参考**:
  - tower-http crate: https://docs.rs/tower-http/latest/tower_http/

  **验收标准**:
  - [ ] CORS headers 正确添加
  - [ ] 请求日志包含方法、路径、状态码
  - [ ] 超时配置生效

  **QA 场景**:

  ```
  场景：验证 CORS 中间件
    工具：Bash (curl)
    前置条件：中间件已配置
    步骤:
      1. 发送 OPTIONS 预检请求
      2. 断言响应包含 Access-Control-Allow-Origin
      3. 断言包含 Access-Control-Allow-Methods
    预期结果：CORS headers 正确
    证据：.sisyphus/evidence/task-18-cors.txt
  ```

  **证据捕获**:
  - [ ] curl -v 输出

  **提交**: YES (与 T17 分组)
  - 消息：`feat(middleware): configure CORS and tracing middleware`
  - 文件：`src/routes/middleware.rs`
  - 预提交：`curl -v -X OPTIONS http://localhost:5564/v1/chat/completions`

---

- [ ] 19. main.rs 应用组装

  **做什么**:
  - 更新 `src/main.rs`
  - 创建 `create_app()` 函数:
    - 加载配置
    - 验证配置
    - 初始化 tracing
    - 创建 ProviderRegistry
    - 配置路由和中间件
    - 返回 axum::Router
  - 在 main 函数中:
    - 调用 create_app()
    - 绑定端口 5564
    - 启动 Tokio 运行时

  **必须不做**:
  - 不实现业务逻辑（仅组装）

  **推荐 Agent Profile**:
  - **Category**: `quick`
    - 原因：应用组装，无复杂逻辑
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: NO
  - **并行组**: Wave 3 收尾
  - **阻塞**: T20-T23
  - **被阻塞**: T1, T3, T5, T14-T18

  **参考**:
  - Python main.py
  - axum 应用结构

  **验收标准**:
  - [ ] `cargo run` 成功启动服务器
  - [ ] 监听端口 5564
  - [ ] 所有路由可访问

  **QA 场景**:

  ```
  场景：验证应用启动
    工具：Bash (cargo run)
    前置条件：所有组件已实现
    步骤:
      1. 运行 cargo run
      2. 检查输出包含 "listening on port 5564"
      3. 在另一个终端发送测试请求
      4. 检查收到响应
    预期结果：服务器成功启动并响应请求
    证据：.sisyphus/evidence/task-19-app-start.txt
  ```

  **证据捕获**:
  - [ ] 服务器启动日志
  - [ ] 测试请求响应

  **提交**: YES (单独提交)
  - 消息：`feat(main): assemble application and start server`
  - 文件：`src/main.rs`
  - 预提交：`cargo run &`

---

- [ ] 20. 单元测试编写

  **做什么**:
  - 为以下模块编写单元测试:
    - config: OneOrMany 反序列化
    - adapter: 适配器链执行顺序
    - types: 序列化/反序列化
    - error: 错误转换
  - 使用 `#[cfg(test)]` 模块
  - 使用 mockall 进行 mock

  **必须不做**:
  - 不写集成测试（这是 T21 的任务）

  **推荐 Agent Profile**:
  - **Category**: `deep`
    - 原因：需要理解所有模块的测试策略
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: NO
  - **并行组**: Wave 4 起始
  - **阻塞**: F1-F4
  - **被阻塞**: T7-T18

  **参考**:
  - Metis 报告 "测试策略" 部分
  - Rust 测试文档

  **验收标准**:
  - [ ] 所有核心模块有单元测试
  - [ ] `cargo test` 通过率 100%
  - [ ] 测试覆盖率 > 80%

  **QA 场景**:

  ```
  场景：运行所有单元测试
    工具：Bash (cargo test)
    前置条件：测试已编写
    步骤:
      1. 运行 cargo test --lib
      2. 检查所有测试通过
      3. 运行 cargo tarpauline --out Html（如果可用）
    预期结果：所有测试通过，覆盖率达标
    证据：.sisyphus/evidence/task-20-unit-tests.txt
  ```

  **证据捕获**:
  - [ ] 测试输出
  - [ ] 覆盖率报告（可选）

  **提交**: YES (单独提交)
  - 消息：`test(unit): add comprehensive unit tests for all modules`
  - 文件：`src/**/*.rs` (测试模块)
  - 预提交：`cargo test --lib`

---

- [ ] 21. 集成测试编写

  **做什么**:
  - 创建 `tests/integration/` 目录
  - 编写端到端测试:
    - 测试完整请求流程
    - 测试流式请求
    - 测试错误处理
    - 测试适配器链
  - 使用 wiremock 模拟上游 provider
  - 使用 test-log 配置测试日志

  **必须不做**:
  - 不使用真实 API key（使用 mock）

  **推荐 Agent Profile**:
  - **Category**: `deep`
    - 原因：需要理解完整请求流程
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 4 (与 T22)
  - **阻塞**: F1-F4
  - **被阻塞**: T14-T19

  **参考**:
  - Metis 报告 "测试策略" 部分
  - wiremock crate: https://docs.rs/wiremock/latest/wiremock/

  **验收标准**:
  - [ ] 集成测试覆盖所有路由
  - [ ] 使用 mock 服务器模拟上游
  - [ ] 测试流式和非流式

  **QA 场景**:

  ```
  场景：运行集成测试
    工具：Bash (cargo test)
    前置条件：集成测试已编写
    步骤:
      1. 运行 cargo test --test '*'
      2. 检查所有集成测试通过
    预期结果：所有集成测试通过
    证据：.sisyphus/evidence/task-21-integration-tests.txt
  ```

  **证据捕获**:
  - [ ] 测试输出

  **提交**: YES (单独提交)
  - 消息：`test(integration): add end-to-end integration tests`
  - 文件：`tests/integration/*.rs`
  - 预提交：`cargo test --test '*'`

---

- [ ] 22. 端到端手动测试

  **做什么**:
  - 使用真实配置启动服务器
  - 使用 curl 或 httpx 发送真实请求
  - 测试以下场景:
    - OpenAI 格式非流式请求
    - OpenAI 格式流式请求
    - Anthropic 格式非流式请求
    - Anthropic 格式流式请求
    - 错误请求（无效 provider，无效模型）
  - 记录所有测试结果

  **必须不做**:
  - 不使用 mock（这是真实测试）

  **推荐 Agent Profile**:
  - **Category**: `unspecified-high`
    - 原因：需要手动验证所有场景
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: YES
  - **并行组**: Wave 4 (与 T21)
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
    前置条件：服务器运行，配置真实 provider
    步骤:
      1. curl -N -X POST http://localhost:5564/v1/chat/completions
      2. Body: {"model": "qwen-code@coder-model", "messages": [...], "stream": true}
      3. 断言收到 SSE 流
      4. 断言流以 [DONE] 结束
    预期结果：流式响应正确
    证据：.sisyphus/evidence/task-22-e2e-stream.txt
  ```

  **证据捕获**:
  - [ ] 所有测试场景的 curl 输出
  - [ ] 错误场景的响应

  **提交**: YES (单独提交)
  - 消息：`test(e2e): perform manual end-to-end testing`
  - 文件：`tests/e2e/README.md` (测试记录)
  - 预提交：手动验证

---

- [ ] 23. 性能基准测试

  **做什么**:
  - 创建 `benches/` 目录
  - 使用 criterion 进行基准测试:
    - 配置解析性能
    - 适配器链执行性能
    - HTTP 请求转发延迟
  - 生成基准报告

  **必须不做**:
  - 不优化过早（先保证正确性）

  **推荐 Agent Profile**:
  - **Category**: `quick`
    - 原因：标准基准测试配置
  - **Skills**: `[]`

  **并行化**:
  - **可并行**: NO
  - **并行组**: Wave 4 收尾
  - **阻塞**: F1-F4
  - **被阻塞**: T19

  **参考**:
  - criterion crate: https://docs.rs/criterion/latest/criterion/

  **验收标准**:
  - [ ] 基准测试可运行
  - [ ] 生成 HTML 报告
  - [ ] 记录关键性能指标

  **QA 场景**:

  ```
  场景：运行基准测试
    工具：Bash (cargo bench)
    前置条件：基准测试已编写
    步骤:
      1. 运行 cargo bench
      2. 检查生成 HTML 报告
      3. 查看关键指标
    预期结果：基准测试完成，报告生成
    证据：.sisyphus/evidence/task-23-benchmarks.txt
  ```

  **证据捕获**:
  - [ ] 基准报告路径

  **提交**: YES (单独提交)
  - 消息：`perf(bench): add criterion benchmarks for key operations`
  - 文件：`benches/*.rs`
  - 预提交：`cargo bench`

---

## Final Verification Wave

> 4 个审查 Agent 并行执行，全部必须 APPROVE。任一拒绝 → 修复 → 重新运行

- [ ] F1. **计划合规审计** — `oracle`
  逐条阅读计划。对每个"Must Have"：验证实现存在（读文件、curl 端点、运行命令）。对每个"Must NOT Have"：搜索代码库查找禁止模式——如果找到则拒绝并返回 file:line。检查证据文件是否存在于 .sisyphus/evidence/。比较交付物与计划。
  输出：`Must Have [N/N] | Must NOT Have [N/N] | Tasks [N/N] | VERDICT: APPROVE/REJECT`

- [ ] F2. **代码质量审查** — `unspecified-high`
  运行 `cargo check` + `cargo clippy` + `cargo test`。审查所有变更文件：`as any`/`#[allow(dead_code)]`、空 catch、生产环境的 println、注释掉的代码、未使用的 import。检查 AI slop：过度注释、过度抽象、通用名称（data/result/item/temp）。
  输出：`Build [PASS/FAIL] | Clippy [PASS/FAIL] | Tests [N pass/N fail] | Files [N clean/N issues] | VERDICT`

- [ ] F3. **真实手动 QA** — `unspecified-high` (+ `playwright` 如果 UI)
  从干净状态开始。执行每个任务的每个 QA 场景——遵循确切步骤，捕获证据。测试跨任务集成（功能协同工作，不是隔离）。测试边界情况：空状态、无效输入、快速操作。保存到 `.sisyphus/evidence/final-qa/`。
  输出：`Scenarios [N/N pass] | Integration [N/N] | Edge Cases [N tested] | VERDICT`

- [ ] F4. **范围保真检查** — `deep`
  对每个任务：读"做什么"，读实际 diff（git log/diff）。验证 1:1——规格中的所有内容都已构建（无缺失），规格之外的内容未构建（无范围蔓延）。检查"必须不做"合规性。检测跨任务污染：任务 N 触碰任务 M 的文件。标记未说明的变更。
  输出：`Tasks [N/N compliant] | Contamination [CLEAN/N issues] | Unaccounted [CLEAN/N files] | VERDICT`

---

## Commit Strategy

- **Wave 1**: `chore(project): setup scaffolding and dependencies` — Cargo.toml, src/lib.rs, src/main.rs, src/error.rs, src/types/mod.rs, src/utils/logging.rs
- **Wave 2**: `feat(config): implement config system` — src/config/*.rs
- **Wave 2**: `feat(adapter): implement adapter trait and chain` — src/adapter/*.rs
- **Wave 2**: `feat(provider): implement HTTP client and registry` — src/provider/*.rs, src/types/request.rs
- **Wave 3**: `feat(routes): implement OpenAI and Anthropic routes` — src/routes/openai.rs, src/routes/anthropic.rs
- **Wave 3**: `feat(sse): implement SSE streaming` — src/utils/sse.rs
- **Wave 3**: `feat(routes): add health and models endpoints` — src/routes/health.rs, src/routes/models.rs
- **Wave 3**: `feat(middleware): configure CORS and tracing` — src/routes/middleware.rs
- **Wave 3**: `feat(main): assemble application` — src/main.rs
- **Wave 4**: `test(unit): add unit tests` — src/**/*.rs (test modules)
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
  -d '{"model": "qwen-code@coder-model", "messages": [{"role": "user", "content": "Hello"}]}'
                                      # 预期：返回 OpenAI 格式响应
```

### 最终检查清单
- [ ] 所有 "Must Have" 已实现
- [ ] 所有 "Must NOT Have" 不存在
- [ ] 所有测试通过
- [ ] 所有 QA 场景证据已捕获
- [ ] Final Verification Wave 全部 APPROVE
