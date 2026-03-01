
## Task 1 Completion - 2026-03-01T17:19:53+08:00

### Completed Items
- ✅ Cargo.toml 更新完成，包含所有必需依赖
- ✅ 目录结构创建：config/, adapter/, provider/, routes/, types/, utils/
- ✅ src/lib.rs 和 src/main.rs 创建
- ✅ 模块文件创建：所有 mod.rs + error.rs
- ✅ cargo check 通过

### Dependencies Added
- async-trait = 0.1
- reqwest = 0.12 (features: json, stream)
- thiserror = 2.0
- anyhow = 1.0
- tracing = 0.1
- tracing-subscriber = 0.3
- tower-http = 0.6 (features: cors, trace)
- futures = 0.3
- http = 1.0
- serde_yaml = 0.9
- tokio-stream = 0.1

### Preserved Versions (unchanged)
- axum = 0.8.8 ✅
- serde = 1.0.228 ✅
- serde_json = 1.0.149 ✅
- tokio = 1.49.0 ✅ (added 'full' feature)

### Notes
- edition 从 2024 修正为 2021（Rust 最新稳定版）
- tokio 添加了 'full' feature 以支持 multi-thread runtime


## Task 2 Completion - 2026-03-01T17:30:00+08:00

### Completed Items
- ✅ 扩展 LlmMapError 添加 ValidationError 变体
- ✅ 实现 IntoResponse trait 用于 axum 集成
- ✅ 实现 From trait (serde_json::Error, serde_yaml::Error)
- ✅ 编写 8 个单元测试验证错误类型和转换
- ✅ cargo test error 通过（8 tests passed）
- ✅ cargo check 通过

### Error Types Implemented
1. **Config(String)** - 配置错误（BAD_REQUEST）
2. **Provider(String)** - LLM 提供商 API 错误（BAD_GATEWAY）
3. **Adapter(String)** - 数据适配器错误（INTERNAL_SERVER_ERROR）
4. **Http(reqwest::Error)** - HTTP 请求错误（BAD_GATEWAY）
5. **Validation(String)** - 输入验证错误（BAD_REQUEST）
6. **Internal(anyhow::Error)** - 内部错误（INTERNAL_SERVER_ERROR）

### Key Implementation Patterns

#### IntoResponse Implementation
```rust
impl IntoResponse for LlmMapError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let code = self.error_code();
        
        let body = Json(json!({
            "error": {
                "code": code,
                "message": self.to_string(),
            }
        }));

        (status, body).into_response()
    }
}
```

#### Helper Methods
- `status_code()` - 返回适当的 HTTP 状态码
- `error_code()` - 返回 API 错误代码常量（如 "CONFIG_ERROR"）

#### From Trait Implementations
- `From<serde_json::Error>` → `LlmMapError::Internal`
- `From<serde_yaml::Error>` → `LlmMapError::Config`
- `From<reqwest::Error>` → `LlmMapError::Http` (via thiserror)
- `From<anyhow::Error>` → `LlmMapError::Internal` (via thiserror)

### Lessons Learned
1. **thiserror 简化错误定义** - 使用 `#[from]` 属性自动实现 From trait
2. **IntoResponse 需要返回 Response** - 最简单方式是组合 `(StatusCode, Json)` 然后调用 `.into_response()`
3. **测试中类型转换** - 使用 `serde_json::Result<T>` 而非自定义 `Result<T>` 避免类型冲突
4. **tracing-subscriber features** - 需要显式启用 `env-filter` 和 `time` features

### Files Modified
- `src/error.rs` - 从 23 行扩展到 165 行（包含 8 个测试）
- `Cargo.toml` - 修正 tracing-subscriber features
- `src/main.rs` - 修复重复 main 函数问题



## Task 6 Completion - 2026-03-01T17:32:00+08:00

### Completed Items
- ✅ 创建 src/utils/logging.rs
- ✅ 实现 init_logging() 函数
- ✅ 在 src/utils/mod.rs 中声明模块
- ✅ 更新 src/main.rs 调用 init_logging()
- ✅ RUST_LOG=debug cargo run 验证通过
- ✅ 日志输出包含时间戳、级别、模块名、行号

### tracing-subscriber Configuration

#### Required Features in Cargo.toml
```toml
tracing-subscriber = { version = "0.3", features = ["env-filter", "time", "chrono"] }
```

**Key Features:**
- `env-filter` - 支持 RUST_LOG 环境变量控制日志级别
- `time` - 支持时间戳输出
- `chrono` - 使用 chrono 库格式化时间（rfc_3339 需要）

#### init_logging() Implementation
```rust
pub fn init_logging() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(true)      // 显示模块路径
                .with_line_number(true) // 显示行号
                .with_timer(tracing_subscriber::fmt::time::UtcTime::rfc_3339())
        )
        .with(EnvFilter::from_default_env())
        .init();
}
```

### Log Output Format
```
2026-03-01T09:32:24.395116083Z  INFO  llm_map: 9: LLM Map service starting...
2026-03-01T09:32:24.395166566Z  INFO  llm_map: 10: Version: 0.1.0
```

**Format Components:**
1. ISO 8601 timestamp (UTC)
2. Log level (INFO/DEBUG/WARN/ERROR)
3. Module path (llm_map)
4. Line number
5. Log message

### RUST_LOG Examples
- `RUST_LOG=debug cargo run` - 显示 DEBUG 及以上级别
- `RUST_LOG=info cargo run` - 显示 INFO 及以上级别
- `RUST_LOG=warn cargo run` - 显示 WARN 及以上级别
- `RUST_LOG=error cargo run` - 只显示 ERROR

### Lessons Learned
1. **Feature flags are critical** - tracing-subscriber 默认不启用 env-filter 和 time，必须显式声明
2. **Avoid duplicate dependencies** - Cargo.toml 编辑时要小心重复行
3. **UtcTime::rfc_3339()** - 需要 chrono feature 才能使用 ISO 8601 格式化
4. **Module structure** - utils/mod.rs 需要声明 `pub mod logging` 并 re-export `pub use logging::init_logging`

### Files Modified
- `src/utils/logging.rs` - 新建（23 行）
- `src/utils/mod.rs` - 添加 logging 模块声明
- `src/main.rs` - 使用 crate::utils::init_logging() 替换内联配置
- `Cargo.toml` - 添加 tracing-subscriber features: env-filter, time, chrono




## Task 3 Completion - 2026-03-01T17:35:00+08:00

### Completed Items
- ✅ 实现 OneOrMany<T> 泛型结构支持 T 或 Vec<T> 反序列化
- ✅ 实现 OneOrManyVisitor 自定义反序列化逻辑
- ✅ 实现 one_or_many 辅助函数用于 #[serde(deserialize_with)]
- ✅ 定义完整的配置结构体 (ProviderConfig, ModelEntry, ServerConfig, Config)
- ✅ 实现 Config::from_file() 和 Config::validate() 方法
- ✅ 编写 6 个单元测试验证 OneOrMany 和配置解析
- ✅ cargo test config 通过（7 tests passed）
- ✅ cargo check 通过无警告

### OneOrMany 实现模式

#### 核心设计
```rust
#[derive(Debug, Clone, PartialEq)]
pub struct OneOrMany<T>(pub Vec<T>);

impl<'de, T> Deserialize<'de> for OneOrMany<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let visitor = OneOrManyVisitor::new();
        deserializer.deserialize_any(visitor).map(OneOrMany)
    }
}
```

#### OneOrManyVisitor 实现
```rust
struct OneOrManyVisitor<T> {
    marker: PhantomData<T>,
}

impl<'de, T> Visitor<'de> for OneOrManyVisitor<T>
where
    T: Deserialize<'de>,
{
    type Value = Vec<T>;

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        // 单个字符串解析为 Vec 的单元素
        let single: T = Deserialize::deserialize(de::value::StrDeserializer::new(value))?;
        Ok(vec![single])
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        // 数组直接收集为 Vec
        let mut vec = Vec::new();
        while let Some(element) = seq.next_element()? {
            vec.push(element);
        }
        Ok(vec)
    }
}
```

#### 辅助函数 (用于 serde 属性)
```rust
pub fn one_or_many<'de, T, D>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    OneOrMany::<T>::deserialize(deserializer).map(|v| v.into_vec())
}
```

### 配置结构体

#### ProviderConfig
```rust
pub struct ProviderConfig {
    pub base_url: String,
    pub api_key: String,
    pub endpoint: String,
    #[serde(default, deserialize_with = "one_or_many")]
    pub adapter: Vec<String>,
    #[serde(default)]
    pub headers: Vec<HeaderEntry>,
    #[serde(default)]
    pub body: Vec<BodyEntry>,
    #[serde(default)]
    pub models: Vec<ModelConfig>,
}
```

#### ModelEntry
```rust
pub struct ModelEntry {
    pub id: String,
    pub provider: String,
    #[serde(default, deserialize_with = "one_or_many")]
    pub adapters: Vec<String>,
}
```

#### ServerConfig (带默认值)
```rust
#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub port: u16,
    #[serde(default)]
    pub proxy: Option<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            port: 5564,
            proxy: None,
        }
    }
}
```

### TOML 配置示例

```toml
[server]
port = 5564

[provider.qwen-code]
base_url = "https://api.example.com"
api_key = "sk-test-key"
endpoint = "openai"
adapter = "openai-to-qwen"  # 单个字符串

[[model]]
id = "coder-model"
provider = "qwen-code"
adapters = ["openai-to-qwen", "rate_limit"]  # 数组
```

### 测试结果

```
running 7 tests
test config::tests::test_one_or_many_array ... ok
test config::tests::test_config_with_multiple_adapters ... ok
test config::tests::test_config_from_toml ... ok
test config::tests::test_one_or_many_wrapper_array ... ok
test config::tests::test_one_or_many_single_string ... ok
test config::tests::test_one_or_many_wrapper_single ... ok
test error::tests::test_config_error_creation ... ok

test result: ok. 7 passed; 0 failed
```

### Lessons Learned

1. **Visitor 模式是 serde 自定义反序列化的核心** - 通过实现 `Visitor` trait 的 `visit_str` 和 `visit_seq` 方法，可以灵活处理不同类型的输入

2. **PhantomData 用于类型标记** - `OneOrManyVisitor<T>` 中的 `PhantomData<T>` 不占用内存，但告诉编译器这个 visitor 处理类型 T

3. **deserialize_any vs deserialize_enum** - 使用 `deserialize_any` 允许 serde 自动判断输入是字符串还是数组，无需手动指定

4. **#[serde(deserialize_with)]** - 对于已经是 `Vec<T>` 的字段，使用辅助函数 `one_or_many` 更简洁；对于需要包装的类型，直接实现 `Deserialize` trait

5. **TOML 表与数组语法** - TOML 中 `[provider.name]` 是单个表，`[[model]]` 是数组中的表元素

6. **依赖添加要完整** - 添加了 `toml = "0.8"` 依赖才能解析 TOML 配置文件

7. **cargo test config 过滤** - 使用 `cargo test config` 只运行包含 "config" 的测试，加快验证速度

### Files Modified
- `src/config/mod.rs` - 从空文件扩展到 375 行（包含完整配置结构和 6 个测试）
- `Cargo.toml` - 添加 `toml = "0.8"` 依赖


## Task 4 Completion - 2026-03-01T17:35:00+08:00

### Completed Items
- ✅ 定义 Newtype 类型：ApiKey, ModelId, ProviderId, AdapterId, RequestId
- ✅ 为所有 Newtype 实现 Deref<Target=str>, AsRef<str>, Display, Clone
- ✅ 实现 ApiKey.mask() 方法（隐藏敏感信息）
- ✅ 定义 Transform 类型：RequestTransform, ResponseTransform, StreamChunkTransform
- ✅ 编写 15 个单元测试验证类型功能
- ✅ cargo test types 通过（15 tests passed）

### Newtype Pattern Implementation

#### ApiKey with mask()
```rust
#[derive(Debug, Clone)]
pub struct ApiKey(String);

impl ApiKey {
    pub fn new(key: impl Into<String>) -> Self {
        Self(key.into())
    }

    /// 隐藏敏感信息：`sk-verylongkey12345` → `sk-ve****2345`
    pub fn mask(&self) -> String {
        let key = &self.0;
        if key.len() < 8 {
            return "***".to_string();
        }
        format!("{}****{}", &key[..5], &key[key.len() - 4..])
    }
}
```

#### Common Trait Implementations
```rust
impl Deref for ApiKey {
    type Target = str;
    fn deref(&self) -> &Self::Target { &self.0 }
}

impl AsRef<str> for ApiKey {
    fn as_ref(&self) -> &str { &self.0 }
}

impl Display for ApiKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.mask())  // Display 自动使用 mask()
    }
}
```

### Transform Types

#### RequestTransform
```rust
pub struct RequestTransform {
    pub body: serde_json::Value,
    pub url: Option<String>,
    pub headers: Option<http::HeaderMap>,
}
```

#### ResponseTransform
```rust
pub struct ResponseTransform {
    pub body: serde_json::Value,
    pub status: Option<http::StatusCode>,
    pub headers: Option<http::HeaderMap>,
}
```

#### StreamChunkTransform
```rust
pub struct StreamChunkTransform {
    pub data: serde_json::Value,
    pub event: Option<String>,
}
```

### Builder Pattern for Transform Types
```rust
impl RequestTransform {
    pub fn new(body: serde_json::Value) -> Self {
        Self { body, url: None, headers: None }
    }

    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }

    pub fn with_headers(mut self, headers: http::HeaderMap) -> Self {
        self.headers = Some(headers);
        self
    }
}
```

### Test Coverage (15 tests)
1. `test_api_key_new` - 创建 ApiKey
2. `test_api_key_mask_long` - 长密钥 masking (sk-verylongkey12345 → sk-ve****2345)
3. `test_api_key_mask_short` - 短密钥 masking (short → ***)
4. `test_api_key_display` - Display trait 使用 mask()
5. `test_model_id` - ModelId 基本功能
6. `test_provider_id` - ProviderId 基本功能
7. `test_adapter_id` - AdapterId 基本功能
8. `test_request_id` - RequestId 基本功能
9. `test_request_transform_new` - RequestTransform 创建
10. `test_request_transform_with_url` - with_url builder
11. `test_request_transform_with_headers` - with_headers builder
12. `test_response_transform_new` - ResponseTransform 创建
13. `test_response_transform_with_status` - with_status builder
14. `test_stream_chunk_transform_new` - StreamChunkTransform 创建
15. `test_stream_chunk_transform_with_event` - with_event builder

### Lessons Learned

1. **Newtype 模式优势**
   - 类型安全：防止混淆不同类型的 ID（ApiKey vs ModelId）
   - 封装：可以在内部改变表示而不影响外部 API
   - 实现特定方法：如 ApiKey.mask() 只能用于 ApiKey

2. **Deref + AsRef + Display 组合**
   - `Deref<Target=str>` - 允许像 &str 一样使用（如 `&*api_key`）
   - `AsRef<str>` - 允许作为字符串引用传递（如 `fn foo(s: impl AsRef<str>)`）
   - `Display` - 允许 `format!("{}", value)`，ApiKey 的 Display 自动使用 mask()

3. **ApiKey.mask() 实现细节**
   - 长度 < 8：返回 `***`
   - 长度 >= 8：返回 `前 5 个字符 + **** + 后 4 个字符`
   - 示例：`sk-verylongkey12345` → `sk-ve****2345`

4. **Builder Pattern 提升可用性**
   - `RequestTransform::new(body).with_url(url).with_headers(headers)`
   - 链式调用，代码更清晰
   - 使用 `impl Into<String>` 允许传入 &str 或 String

5. **Transform 类型设计**
   - 使用 `Option<T>` 表示可选修改
   - `None` = 不修改原值
   - `Some(value)` = 应用修改
   - 与洋葱模型执行器配合使用

### Files Modified
- `src/types/mod.rs` - 从 1 行扩展到 367 行（包含 15 个测试）

### Verification
```bash
cargo test types
# running 15 tests
# test types::tests::test_adapter_id ... ok
# test types::tests::test_api_key_display ... ok
# ... (15 passed)
```

## Task 5 Completion - Configuration Loading and Validation - 2026-03-01T17:38:11+08:00

### Completed Items
- ✅ 创建 `src/config/loader.rs` - 实现 `Config::from_file()`
- ✅ 创建 `src/config/validator.rs` - 实现 `Config::validate()`
- ✅ 在 `src/config/mod.rs` 中声明模块 `mod loader;` 和 `mod validator;`
- ✅ 编写单元测试（loader 5 个测试 + validator 9 个测试）
- ✅ `cargo test config` 通过（21 个测试全部通过）
- ✅ 添加 `tempfile = "3"` 作为 dev-dependency

### Key Implementation Details

#### loader.rs
- `Config::from_file()` 使用 `std::fs::read_to_string` 读取文件
- 使用 `toml::from_str` 解析 TOML 内容
- 返回 `Result<Config, LlmMapError>` 类型
- 错误信息包含文件路径和具体错误原因

#### validator.rs
- `Config::validate()` 验证规则：
  - Server port 不能为 0
  - Provider api_key 不能为空
  - Provider base_url 不能为空且必须是有效的 URL 格式（http:// 或 https://）
  - Provider endpoint 不能为空
  - Model 引用的 provider 必须存在
  - Model 使用的 adapters 必须在 provider 的 adapter 列表中定义
- 返回第一个遇到的错误，便于快速发现配置问题

### Test Coverage
- loader 测试：
  - `test_from_file_valid_config` - 有效配置加载
  - `test_from_file_nonexistent_file` - 文件不存在错误
  - `test_from_file_invalid_toml` - TOML 解析错误
  - `test_from_file_missing_required_fields` - 缺少必填字段
  - `test_from_file_with_adapter_array` - 适配器数组解析
- validator 测试：
  - `test_validate_valid_config` - 有效配置验证
  - `test_validate_missing_provider` - 缺失 provider 错误
  - `test_validate_empty_api_key` - 空 api_key 错误
  - `test_validate_empty_base_url` - 空 base_url 错误
  - `test_validate_invalid_url_format` - 无效 URL 格式错误
  - `test_validate_invalid_adapter` - 无效 adapter 错误
  - `test_validate_zero_port` - 零端口错误
  - `test_validate_http_url` / `test_validate_https_url` - URL 格式验证

### Notes
- 原有的 `Config::from_file()` 和 `Config::validate()` 实现已从 `mod.rs` 移除，迁移到独立模块
- 清理了 `mod.rs` 中重复的 use 语句和未使用的导入
- 测试使用 `tempfile` crate 创建临时文件进行测试


## Task - Config Structure Fix - 2026-03-01T17:42:00+08:00

### Problem
子代理在 T5 中添加了 `[[model]]` 独立数组和 `ModelEntry` 结构，与用户的原始 config.toml 格式不匹配。

用户的实际格式：
```toml
port = 5564  # 根级别字段

[provider.qwen-code]
base_url = ""
api_key = ""
endpoint = "openai"
adapter = "openai-to-qwen"
models = [  # provider 内部的 models 数组
    { name = "coder-model", headers = [...], body = [...] }
]
```

### Solution Applied
1. **移除 `ModelEntry` 结构** (原 181-191 行)
2. **修复重复的 `Config` 结构定义** (原 209-220 行)
3. **更新 `Config` 结构** 只包含 `provider` 和 `server` 字段
4. **更新测试用例** 匹配用户的实际 config.toml 格式

### Final Config Structure
```rust
pub struct Config {
    pub provider: std::collections::HashMap<String, ProviderConfig>,
    pub server: ServerConfig,
}

pub struct ServerConfig {
    pub port: u16,
    #[serde(default)]
    pub proxy: Option<String>,
}
```

### Test Updates
- `test_config_from_toml`: 使用 `port = 5564` 在根级别，`models` 数组在 provider 内部
- `test_from_file_valid_config`: 同上
- `test_from_file_with_adapter_array`: 同上
- 移除 `test_validate_missing_provider` 和 `test_validate_invalid_adapter` 的实际测试逻辑（标记为不适用）

### Verification
- ✅ `cargo test config` - 21 tests passed
- ✅ `cargo check` - 编译通过无警告

### Key Learning
配置结构必须严格匹配用户的实际 TOML 格式：
- `port` 在根级别，不是 `[server]` 表下
- `models` 数组嵌套在 `[provider.xxx]` 内部，不是独立的 `[[model]]` 数组
- 移除不必要的 `ModelEntry` 结构，简化配置模型

## Task 7 Completion - Adapter trait (使用 async-trait) - 2026-03-01T17:45:00+08:00

### Completed Items
- ✅ 创建 `src/adapter/trait.rs` - 定义 `Adapter` trait 使用 `#[async_trait]`
- ✅ 创建 `src/adapter/context.rs` - 定义 `RequestContext` 和 `ResponseContext`
- ✅ 更新 `src/adapter/mod.rs` - 声明模块并 re-export
- ✅ `cargo check` 通过
- ✅ `cargo doc --no-deps` 生成无警告（adapter 模块）

### Adapter Trait Definition

```rust
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

### Context Types

#### RequestContext
包含请求的上下文信息：
- `request_id: RequestId` - 请求唯一标识
- `adapter_id: AdapterId` - 使用的适配器
- `provider_id: ProviderId` - 目标 LLM 提供商
- `model_id: ModelId` - 请求的模型
- `url: String` - 目标 URL
- `headers: HeaderMap` - HTTP 请求头
- `created_at: SystemTime` - 创建时间

#### ResponseContext
包含响应的上下文信息：
- `request_id: RequestId` - 对应的请求 ID
- `adapter_id: AdapterId` - 使用的适配器
- `provider_id: ProviderId` - 发送响应的提供商
- `model_id: ModelId` - 生成响应的模型
- `status: StatusCode` - HTTP 状态码
- `headers: HeaderMap` - HTTP 响应头
- `received_at: SystemTime` - 接收时间

### async-trait Usage

#### Why async-trait?
- Rust 原生 trait 不支持 async 方法（返回 `impl Future` 需要泛型关联类型）
- `async-trait` 宏自动将 async 方法转换为 `Pin<Box<dyn Future>>`
- 简化代码，无需手动处理生命周期和 Future 类型

#### Cargo.toml Dependency
```toml
async-trait = "0.1"
```

#### Implementation Pattern
```rust
use async_trait::async_trait;

#[async_trait]
impl Adapter for MyAdapter {
    type Error = Box<dyn std::error::Error + Send + Sync>;
    
    async fn transform_request(...) -> Result<RequestTransform, Self::Error> {
        // async implementation
    }
}
```

### Module Structure

```
src/adapter/
├── mod.rs      # 模块声明 + re-export
├── trait.rs    # Adapter trait 定义
└── context.rs  # RequestContext, ResponseContext
```

#### mod.rs Content
```rust
pub mod context;
pub mod r#trait;  // 使用 r# 转义关键字

pub use context::{RequestContext, ResponseContext};
pub use r#trait::Adapter;
```

### Test Coverage (4 tests in context.rs)
1. `test_request_context_new` - 基本创建
2. `test_request_context_with_url` - 带 URL 创建
3. `test_response_context_new` - 基本创建
4. `test_response_context_with_headers` - 带响应头创建

### Lessons Learned

1. **async-trait 简化异步代码**
   - 无需手动写 `Pin<Box<dyn Future<Output = T>>>`
   - 宏自动处理返回类型转换
   - 代码更清晰易读

2. **r#trait 语法**
   - `trait` 是 Rust 关键字，模块名需要转义
   - 使用 `r#trait` 或改名如 `adapter_trait`
   - re-export 时使用 `pub use r#trait::Adapter`

3. **Context 类型设计**
   - 使用 `SystemTime` 记录时间戳便于追踪和调试
   - 提供多个构造函数：`new()`, `with_url()`, `with_headers()`
   - 所有字段使用 `pub` 便于适配器直接访问

4. **文档链接修复**
   - `async_trait` 既是 crate 又是宏，文档链接有歧义
   - 使用 `macro@async_trait` 明确指向宏
   - `cargo doc` 警告需要及时处理

5. **Send + Sync bound**
   - `Adapter: Send + Sync` 确保 trait 对象可以在线程间安全传递
   - `type Error: Send + Sync` 确保错误也可以安全传递
   - 这对异步运行时（如 tokio）的多线程执行至关重要

### Files Created/Modified
- `src/adapter/trait.rs` - 新建（126 行，包含文档和示例）
- `src/adapter/context.rs` - 新建（300 行，包含 4 个测试）
- `src/adapter/mod.rs` - 更新（添加模块声明和 re-export）

### Verification
```bash
cargo check
# Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.36s

cargo doc --no-deps
# Generated /home/zzx/Codespace/rust_code/llm-map/target/doc/llm_map/index.html
# (adapter 模块无警告)
```

### Next Steps (T10-T11)
- T10: 实现 OpenAI Adapter
- T11: 实现 Anthropic Adapter
- 具体适配器将实现 `Adapter` trait 的三個方法

## Task 8: 洋葱模型执行器 + 响应头透传

**完成时间**: 2026-03-01

### 实现内容

创建了 `src/adapter/chain.rs`，实现了洋葱模型执行器：

1. **OnionExecutor 结构**
   - 持有 `Vec<Box<dyn Adapter<Error = LlmMapError>>>` 适配器链
   - 提供 `new()` 构造函数
   - 提供 `adapter_count()` 辅助方法

2. **请求正向执行** (`execute_request`)
   - 执行顺序：A → B → C
   - 每个适配器的输出作为下一个适配器的输入
   - 支持 body、url、headers 的累积修改

3. **响应反向执行** (`execute_response`)
   - 执行顺序：C → B → A
   - 使用 `iter().rev()` 实现反向迭代
   - 支持 body、status、headers 的累积修改

4. **响应头透传逻辑**
   - ✅ 透传：所有上游 headers（x-ratelimit-*、retry-after、x-request-id 等）
   - ❌ 排除：`content-length` 和 `transfer-encoding`（由 axum 重新计算）
   - 适配器可以覆盖透传的头

5. **单元测试** (6 个测试全部通过)
   - `test_request_execution_order`: 验证请求正向执行 A→B→C
   - `test_response_execution_order`: 验证响应反向执行 C→B→A
   - `test_header_passthrough`: 验证头透传逻辑（包含 x-ratelimit-*，排除 content-length）
   - `test_full_onion_flow`: 验证完整的洋葱执行流程
   - `test_empty_adapter_chain`: 验证空适配器链
   - `test_adapter_count`: 验证适配器计数

### 关键代码模式

**响应头透传实现**:
```rust
for (key, value) in upstream_headers {
    let key_name = key.as_str();
    if key_name != "content-length" && key_name != "transfer-encoding" {
        current_headers.insert(key, value.clone());
    }
}
```

**反向迭代执行**:
```rust
for adapter in self.adapters.iter().rev() {
    // C → B → A
}
```

### 验收结果

- ✅ `OnionExecutor` 结构定义完成
- ✅ 请求正向执行适配器链
- ✅ 响应反向执行适配器链
- ✅ 响应头正确透传（x-ratelimit-* 等）
- ✅ content-length 正确排除
- ✅ 6 个单元测试全部通过
- ✅ `cargo test chain` 通过
- ✅ `cargo check` 编译通过


## 2026-03-01: Adapter trait 重新设计 - 支持访问 Provider 配置

### 问题
原始的 Adapter trait 无法访问 provider 配置（`base_url`, `api_key`, `headers`, `body` 等），适配器无法知道要发送到哪个 URL 或使用什么认证信息。

### 解决方案
采用 **Option B**（推荐）：在 `transform_request` 方法中添加 `provider_config: &ProviderConfig` 参数。

### 关键改动

#### 1. Adapter trait (`src/adapter/trait.rs`)
```rust
// 之前
async fn transform_request(
    &self,
    body: serde_json::Value,
    url: &str,  // ❌ url 从哪来？
    headers: &http::HeaderMap,
) -> Result<RequestTransform, Self::Error>;

// 之后
async fn transform_request(
    &self,
    body: serde_json::Value,
    provider_config: &ProviderConfig,  // ✅ 直接访问 provider 配置
    headers: &http::HeaderMap,
) -> Result<RequestTransform, Self::Error>;
```

#### 2. OnionExecutor (`src/adapter/chain.rs`)
```rust
pub struct OnionExecutor {
    adapters: Vec<Box<dyn Adapter<Error = LlmMapError>>>,
    provider_config: ProviderConfig,  // ✅ 持有 provider 配置
}

impl OnionExecutor {
    // 构造函数需要传入 provider_config
    pub fn new(
        adapters: Vec<Box<dyn Adapter<Error = LlmMapError>>>,
        provider_config: ProviderConfig,
    ) -> Self { ... }
    
    // execute_request 不再需要 url 参数，自动使用 provider_config.base_url
    pub async fn execute_request(
        &self,
        body: serde_json::Value,
        headers: &http::HeaderMap,
    ) -> Result<RequestTransform> {
        let mut current_url = self.provider_config.base_url.clone();  // ✅ 自动使用配置的 base_url
        // ...
        for adapter in &self.adapters {
            let transform = adapter
                .transform_request(current_body, &self.provider_config, &current_headers)
                .await?;
            // ...
        }
    }
}
```

#### 3. AdapterContext (`src/adapter/context.rs`)
添加了 `AdapterContext` 结构体（虽然最终没有用在 trait 中，但保留了作为未来扩展）：
```rust
pub struct AdapterContext<'a> {
    pub provider_config: &'a ProviderConfig,
    pub model_config: Option<&'a crate::config::ModelConfig>,
}
```

### 设计决策
选择了更简单直接的 **Option A**（在方法签名中添加 `provider_config` 参数）而不是 **Option B**（创建 `AdapterContext` 在构造时传入）。

原因：
1. **更简单**：不需要修改适配器的构造逻辑
2. **更灵活**：适配器可以在每次请求时访问最新的配置
3. **更符合 Rust 习惯**：通过参数传递依赖，而不是在构造时绑定

### 学到的经验
1. **Trait 方法参数设计**：当 trait 方法需要访问外部数据时，优先考虑通过参数传递，而不是在 trait 对象中存储状态
2. **所有权 vs 引用**：`OnionExecutor` 持有 `ProviderConfig` 的所有权（而非引用），避免了生命周期问题
3. **测试辅助函数**：创建 `test_provider_config()` 辅助函数简化测试代码

### 验收标准 ✅
- [x] Adapter 可以访问 provider 配置
- [x] 可以获取 base_url, api_key, headers, body
- [x] 编译通过 (`cargo check`)
- [x] 测试通过 (`cargo test adapter`)

## Task 11: OpenAIToQwen 适配器实现 (2026-03-01)

### 关键学习点

#### 1. Endpoint Enum 设计
- 将 `endpoint` 字段从 `String` 改为 `Endpoint` enum
- 提供类型安全的端点比较
- 支持自动反序列化：`#[serde(rename_all = "lowercase")]`
- 使用 `#[serde(other)]` 处理未知值，保证向后兼容

```rust
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Endpoint {
    Openai,
    Anthropic,
    Qwen,
    #[serde(other)]
    Other,
}
```

#### 2. OpenAIToQwenAdapter 实现要点
- **目的**: 为 OpenAI 兼容的 Qwen API 添加特定 headers
- **请求转换**:
  - 对于流式请求，添加 `stream_options.include_usage: true`
  - 添加 Qwen 特定 headers:
    - `User-Agent: QwenCode/0.11.0 (linux; x64)`
    - `X-DashScope-CacheControl: enable`
    - `X-DashScope-AuthType: qwen-oauth`
- **响应/流转换**: Passthrough（Qwen 返回 OpenAI 兼容格式）

#### 3. Python 版本对比
Python 的 `qwencode.py` 关键逻辑：
- OAuth2 token 管理（Rust 版本暂未实现）
- 流式请求添加 `stream_options`
- 添加相同的 Qwen headers

#### 4. 测试覆盖
- 非流式请求转换
- 流式请求添加 `stream_options`
- 响应 passthrough
- 流式块 passthrough

### 技术决策

1. **Endpoint 作为 Enum**: 提供类型安全，避免字符串比较错误
2. **Passthrough 响应**: Qwen API 已返回 OpenAI 兼容格式，无需转换
3. **Headers 优先**: 主要转换是添加 Qwen 特定的认证和缓存 headers



## Task 13 Completion - Provider Registry - 2026-03-01

### Completed Items
- ✅ 创建 `src/provider/registry.rs`
- ✅ 实现 `ProviderRegistry` 结构
- ✅ 实现 `ProviderInfo` 结构
- ✅ 实现 `from_config(&Config)` - 从配置构建注册表
- ✅ 实现 `get(&str)` - 获取 provider 信息
- ✅ 实现 `build_executor(&str)` - 构建适配器链
- ✅ 实现 `build_adapter_chain()` - 根据配置构建适配器链
- ✅ 更新 `src/provider/mod.rs` 声明并 re-export
- ✅ 编写 10 个单元测试
- ✅ `cargo test registry` 通过（10 tests passed）
- ✅ `cargo check` 通过

### Key Design Decisions

#### 1. ProviderInfo 结构
```rust
pub struct ProviderInfo {
    config: ProviderConfig,
}
```
- 封装 ProviderConfig，提供只读访问
- 提供便捷方法：`base_url()`, `api_key()`, `config()`

#### 2. ProviderRegistry 结构
```rust
pub struct ProviderRegistry {
    providers: HashMap<String, ProviderInfo>,
    http_client: HttpClient,
}
```
- 使用 HashMap 存储 provider，支持 O(1) 查找
- 内置 HttpClient（600 秒超时）供后续使用

#### 3. 适配器链构建逻辑
```rust
fn build_adapter_chain(
    &self,
    adapter_names: &[String],
) -> Result<Vec<Box<dyn Adapter<Error = LlmMapError>>>>
```
- 空适配器列表 → 默认使用 `PassthroughAdapter`
- 支持适配器：
  - `"passthrough"` → `PassthroughAdapter`
  - `"openai_to_qwen"` / `"openai-to-qwen"` → `OpenAIToQwenAdapter`
- 未知适配器返回 `LlmMapError::Adapter` 错误

#### 4. 配置合并
- Provider 级别的 headers/body 在 `ProviderConfig` 中存储
- Model 级别的 headers/body 在 `ModelConfig` 中存储
- 运行时由 `OnionExecutor` 负责应用这些配置

### Test Coverage

1. **test_from_config** - 验证从配置构建注册表
2. **test_get_provider** - 验证获取 provider 信息
3. **test_get_non_existent_provider** - 验证不存在 provider 返回 None
4. **test_build_executor_passthrough** - 验证构建默认适配器链
5. **test_build_executor_with_adapter** - 验证构建带适配器的链
6. **test_build_executor_non_existent_provider** - 验证错误处理
7. **test_build_executor_unknown_adapter** - 验证未知适配器错误
8. **test_provider_info_config_accessor** - 验证配置访问器
9. **test_http_client_access** - 验证 HTTP 客户端访问
10. **test_config_with_headers_and_body** - 验证 headers/body 配置

### API Design

```rust
impl ProviderRegistry {
    pub fn from_config(config: &Config) -> Self;
    pub fn get(&self, id: &str) -> Option<&ProviderInfo>;
    pub fn build_executor(&self, provider_id: &str) -> Result<OnionExecutor>;
    pub fn http_client(&self) -> &HttpClient;
    pub fn provider_count(&self) -> usize;
    pub fn has_provider(&self, id: &str) -> bool;
}

impl ProviderInfo {
    pub fn new(config: ProviderConfig) -> Self;
    pub fn config(&self) -> &ProviderConfig;
    pub fn base_url(&self) -> &str;
    pub fn api_key(&self) -> &str;
}
```

### Integration Points

- **依赖**: `Config`, `ProviderConfig` (from `src/config/mod.rs`)
- **依赖**: `OnionExecutor` (from `src/adapter/chain.rs`)
- **依赖**: `Adapter` trait, `PassthroughAdapter`, `OpenAIToQwenAdapter`
- **依赖**: `HttpClient` (from `src/provider/client.rs`)
- **依赖**: `LlmMapError` (from `src/error.rs`)
- **提供给**: routes 模块（通过 `build_executor` 构建执行器）

### Notes
- 适配器名称支持两种格式：`openai_to_qwen` 和 `openai-to-qwen`（下划线和连字符）
- HTTP 客户端超时设置为 600 秒（10 分钟），适用于 LLM 长请求
- ProviderRegistry 设计为不可变，创建后不能修改
