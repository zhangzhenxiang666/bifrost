# Core Rules

- 请阅读 `ARCHITECTURE.md` 以了解架构设计。
- 优先使用 `let` 链（将 `if let` 与 `&&` 结合），而非嵌套的 `if let` 语句。
    例如:
    ```rust
    let y = Some(Value::String("y".into()));
    
    // 错误用法
    if let Some(x) = y {
        if x.is_string() {
            ...
        }
    }

    // 正确用法
    if let Some(x) = y
      && x.is_string() {
        ...
    }
    ```
- 务必尝试为行为变更添加测试用例。
- 在一个功能添加并测试成功之后, 运行`cargo clippy`检查代码是否可优化, 最后使用`cargo fmt`格式化。
- 优先运行特定测试，而非运行整个测试套件。
- 避免使用 `panic!`、`unreachable!`、`.unwrap()`、不安全代码以及忽略 Clippy 规则。
- 编写不安全代码时，务必遵循我们通常的风格编写 `SAFETY` 注释。
- 如果必须禁用 Clippy 检查，优先使用 `#[expect()]` 而非 `#[allow()]`。
- 切勿更新锁文件中的所有依赖项，务必使用 `cargo update --precise` 来进行锁文件变更。
- 切勿假设 Clippy 警告是预先存在的，`main` 分支极少存在警告。
- 添加新用例时，务必阅读并仿照类似测试的风格。
- 优先使用顶层导入，而非局部导入或完全限定名。
- 避免缩写变量名，例如使用 `version` 而非 `ver`，使用 `context` 而非 `ctx`。
- 编写 Rust 文档注释时，优先使用 [`TypeName`] 形式的引用。
