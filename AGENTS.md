# AGENTS.md

Project instructions for coding agents working in this repository.

## Working Style

- Before cross-crate, adapter, provider, routing, or config changes, read `ARCHITECTURE.md` to understand crate responsibilities and request flow.
- Think before coding. State assumptions when they affect behavior, and ask when ambiguity would change the implementation.
- Prefer the smallest implementation that solves the request. Do not add speculative features, abstractions, configurability, or error handling.
- Make surgical changes. Touch only files and lines that directly support the requested work.
- Match nearby style, even when it differs from personal preference.
- Do not refactor unrelated code, rewrite comments, or clean up pre-existing dead code unless explicitly asked.
- If your changes make imports, variables, functions, or tests unused, clean up only the unused code introduced by your changes.
- For non-trivial work, define success criteria and verify them with the narrowest useful check.

## Project Boundaries

- The root `bifrost` crate is the CLI and operational entrypoint.
- `bifrost-server` owns HTTP routes, provider execution, adapter chains, protocol conversion, SSE handling, middleware, and server state.
- `bifrost-shared` owns shared config, errors, usage records, and types used across the CLI and server.
- Keep conversion behavior in the adapter or converter modules. Avoid embedding protocol conversion details in route handlers.
- Keep shared data contracts in `bifrost-shared` only when they are needed by more than one crate.

## Rust Style

- Prefer top-level imports over local imports or fully qualified names.
- Prefer descriptive variable names, such as `version` instead of `ver` and `context` instead of `ctx`.
- Prefer Rust 2024 `if let` chains when multiple nested `if let` or `let Some(...)` checks can be expressed clearly.

```rust
let value = Some(serde_json::Value::String("example".into()));

// Avoid this in new code.
if let Some(inner) = value {
    if inner.is_string() {
        // ...
    }
}

// Prefer this when it stays readable.
if let Some(inner) = value
    && inner.is_string()
{
    // ...
}
```

- Do not rewrite existing imports or conditionals solely to satisfy these style rules. Apply them to new code and code already being edited for the task.
- Avoid `panic!`, `unreachable!`, `.unwrap()`, unsafe code, and ignored Clippy rules.
- If unsafe code is required, write the usual `SAFETY` comment that explains the invariant being upheld.
- If a Clippy lint must be disabled, prefer `#[expect(...)]` over `#[allow(...)]`.
- In Rust doc comments, prefer link-style references such as [`TypeName`].

## Tests And Verification

- Try to add tests for behavior changes.
- Read similar tests before adding a new case, then follow their structure and naming style.
- Prefer focused tests over broad test runs when the change is narrow.
- This is a Cargo workspace. For ordinary checks and tests, target the specific crate that changed, such as `cargo test -p bifrost-server`.
- After a feature change is implemented and focused tests pass, run `cargo clippy --workspace` to catch improvement opportunities, then run `cargo fmt --all`.
- Do not assume Clippy warnings are pre-existing. The `main` branch should normally be clean.
- For documentation-only changes, no build or test command is required unless the documentation includes generated examples or checked snippets.

## Dependencies And Lockfile

- Do not update all locked dependencies.
- If a lockfile change is necessary, use a precise update command such as `cargo update -p <crate> --precise <version>`.
- Keep dependency additions scoped to the crate that actually needs them.
