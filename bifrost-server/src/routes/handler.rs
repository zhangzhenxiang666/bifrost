//! Generic route handler utilities for LLM endpoints

use crate::adapter::chain::OnionExecutor;
use crate::error::{LlmMapError, Result};
use crate::model::RequestTransform;
use crate::state::AppState;
use crate::types::AliasEntry;
use crate::util;
use axum::response::IntoResponse;
use axum::response::sse::Event;
use bifrost_shared::Endpoint;
use eventsource_stream::Eventsource;
use http::{HeaderMap, header};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;

use crate::routes::{RouteEndpoint, build_request_parts};
use bifrost_shared::usage::{record_stream_usage, record_usage};

/// Context for processing provider responses
pub struct RequestContext {
    pub url: String,
    pub body: Value,
    pub headers: HeaderMap,
    pub executor: OnionExecutor,
    /// Upstream provider endpoint type (openai or anthropic)
    pub provider_endpoint: Endpoint,
    /// Provider ID from config
    pub provider_id: String,
    /// Model name being called
    pub model_name: String,
}

/// Execute a provider request and return the transformed response
///
/// # Provider Configuration Handling
///
/// This function processes provider configuration in the following priority order (highest to lowest):
/// 1. **Model-specific config**: Applies model-specific body/headers from `provider.models` (if matched)
/// 2. **Provider-level config**: Merges `provider.body` and `provider.headers`
/// 3. **Alias resolution**: If model not in provider@model format, resolve via alias
///    - Extract target string from AliasEntry (Simple or Complex)
///    - If Complex alias with headers/body, merge them into the request
/// 4. **Internal adapter**: Adapter is dynamically created based on endpoint type (no user config needed)
pub async fn execute_provider_request(
    state: &AppState,
    route: RouteEndpoint,
    mut headers: HeaderMap,
    mut body: Value,
) -> Result<RequestContext> {
    let model_value = body
        .get("model")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| LlmMapError::Validation("Missing required field: model".to_string()))?;

    // If model is not in provider@model format, try to resolve via global alias
    let (model_target, alias_extra_headers, alias_extra_body) = if model_value.contains('@') {
        // Direct provider@model format, no mapping needed
        (model_value.as_str(), None, None)
    } else {
        let alias_entry = state.registry.get_alias_entry(&model_value);

        match alias_entry {
            Some(AliasEntry::Simple(target)) => (target.as_str(), None, None),
            Some(AliasEntry::Complex(config)) => {
                // Extract extra headers and body from complex mapping
                let extra_headers = config.headers.clone();
                let extra_body = config.body.clone();
                (config.target.as_str(), extra_headers, extra_body)
            }
            None => {
                return Err(LlmMapError::Validation(format!(
                    "Unknown model '{}'. Expected format: provider@model (e.g., 'openai@gpt-4o')",
                    model_value
                )));
            }
        }
    };

    let (provider_id, model_name) = util::parse_model(model_target)?;

    let provider = state
        .registry
        .get(provider_id)
        .ok_or_else(|| LlmMapError::Provider(format!("Provider '{}' not found", provider_id)))?;

    // Update model field to use model_name only (without provider prefix)
    // Safety: We just verified model exists
    *body.get_mut("model").unwrap() = Value::String(model_name.to_string());

    let mut final_headers = HeaderMap::new();

    let executor = state.registry.build_executor(provider_id, &route)?;

    let RequestTransform { mut body } = executor.execute_request(body).await?;

    // If extend=true, inherit the original request headers (after removing excluded ones)
    if provider.extend {
        util::remove_excluded_headers(&mut headers, provider.exclude_headers.as_deref());
        util::extend_overwrite(&mut final_headers, headers);
    }

    // Merge alias extra body fields first (before provider-level)
    if let Some(extra_body) = alias_extra_body {
        for body_entry in extra_body {
            body[&body_entry.name] = body_entry.value;
        }
    }

    // Merge alias extra headers (before provider-level headers)
    if let Some(extra_headers) = alias_extra_headers {
        for header_entry in extra_headers {
            if let Ok(header_name) = header_entry.name.parse::<http::header::HeaderName>()
                && let Ok(header_value) = header_entry.value.parse::<http::header::HeaderValue>()
            {
                final_headers.insert(header_name, header_value);
            }
        }
    }

    // Merge provider-configured body fields into the request body
    if let Some(provider_body_fields) = provider.body.as_ref() {
        for body_entry in provider_body_fields {
            body[&body_entry.name] = body_entry.value.clone();
        }
    }

    if let Some(provider_headers) = provider.headers.as_ref() {
        for header_entry in provider_headers {
            if let Ok(header_name) = header_entry.name.parse::<http::header::HeaderName>()
                && let Ok(header_value) = header_entry.value.parse::<http::header::HeaderValue>()
            {
                final_headers.insert(header_name, header_value);
            }
        }
    }

    // Merge model-specific body fields if model is configured in provider.models
    if let Some(models_config) = provider.models.as_ref()
        && let Some(model_cfg) = models_config.iter().find(|m| m.name == model_name)
    {
        // Merge model-specific body fields
        if let Some(model_body_fields) = model_cfg.body.as_ref() {
            for body_entry in model_body_fields {
                body[&body_entry.name] = body_entry.value.clone();
            }
        }
        // Merge model-specific headers
        if let Some(model_headers) = model_cfg.headers.as_ref() {
            for header_entry in model_headers {
                if let Ok(header_name) = header_entry.name.parse::<http::header::HeaderName>()
                    && let Ok(header_value) =
                        header_entry.value.parse::<http::header::HeaderValue>()
                {
                    final_headers.insert(header_name, header_value);
                }
            }
        }
    }

    let (url, auth_headers) = build_request_parts(provider);

    util::extend_overwrite(&mut final_headers, auth_headers);

    Ok(RequestContext {
        url,
        body,
        headers: final_headers,
        executor,
        provider_endpoint: provider.endpoint.clone(),
        provider_id: provider_id.to_string(),
        model_name: model_name.to_string(),
    })
}

fn try_extract_usage(
    chunk: &Value,
    event: &str,
    endpoint: &Endpoint,
    prompt_tokens: &mut u32,
    completion_tokens: &mut u32,
) {
    match endpoint {
        Endpoint::OpenAI => {
            if let Some(usage) = chunk.get("usage").and_then(|u| u.as_object()) {
                *prompt_tokens = usage
                    .get("prompt_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
                *completion_tokens = usage
                    .get("completion_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
            }
        }
        Endpoint::Anthropic => {
            if event == "message_start"
                && let Some(msg) = chunk.get("message").and_then(|m| m.as_object())
                && let Some(usage) = msg.get("usage").and_then(|u| u.as_object())
            {
                *prompt_tokens = usage
                    .get("input_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
            } else if event == "message_delta"
                && let Some(usage) = chunk.get("usage").and_then(|u| u.as_object())
            {
                *completion_tokens = usage
                    .get("output_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
            }
        }
    }
}

/// Process a streaming request and return SSE response
pub async fn process_stream_request(
    state: &AppState,
    ctx: RequestContext,
) -> Result<axum::response::Response> {
    let RequestContext {
        url,
        body,
        mut headers,
        executor,
        provider_endpoint,
        provider_id,
        model_name,
    } = ctx;

    headers.insert(header::ACCEPT, "text/event-stream".parse().unwrap());

    let response = state
        .registry
        .http_client()
        .send_request(&url, body, headers)
        .await
        .map_err(LlmMapError::Http)?;

    let status_code = response.status();
    let mut upstream_headers = response.headers().clone();

    // Strip headers that conflict with axum's auto-generated response headers
    util::remove_excluded_headers(&mut upstream_headers, None);

    if !status_code.is_success() {
        let body = response.bytes().await.map_err(LlmMapError::Http)?;
        return Ok((status_code, upstream_headers, body).into_response());
    }

    // Create channel for real-time streaming
    // Buffer size 32 is enough for real-time streaming without too much memory pressure
    let (tx, rx) = mpsc::channel::<std::result::Result<Event, axum::BoxError>>(32);

    let span = tracing::Span::current();

    // Spawn task to process upstream stream and send events via channel
    tokio::spawn(async move {
        let _guard = span.enter();
        let mut stream = response.bytes_stream().eventsource();
        let mut prompt_tokens: u32 = 0;
        let mut completion_tokens: u32 = 0;

        while let Some(event_result) = stream.next().await {
            match event_result {
                Ok(event) => {
                    // Skip [DONE] sentinel - standard OpenAI format to end stream
                    if event.data.starts_with("[DONE]") {
                        continue;
                    }

                    // Parse chunk - let adapter handle any format
                    let chunk: Value = serde_json::from_str(&event.data)
                        .unwrap_or_else(|_| json!({"raw": event.data}));

                    // Extract usage
                    try_extract_usage(
                        &chunk,
                        &event.event,
                        &provider_endpoint,
                        &mut prompt_tokens,
                        &mut completion_tokens,
                    );

                    // Transform through adapter chain
                    let transform = executor.execute_stream_chunk(chunk, event.event).await;

                    // If transformation failed, skip this chunk (don't send error to client)
                    let Ok(transform) = transform else {
                        continue;
                    };

                    // Convert all events to SSE events and send immediately
                    for (data, event_name) in transform.events {
                        let data_str =
                            serde_json::to_string(&data).unwrap_or_else(|_| "{}".to_string());
                        let mut sse_event = Event::default();
                        // Set event name first, then data - ensures event: comes before data: in SSE output
                        if let Some(name) = event_name {
                            sse_event = sse_event.event(name);
                        }
                        sse_event = sse_event.data(data_str);
                        // Send event - if receiver is dropped, stop processing
                        if tx.send(Ok(sse_event)).await.is_err() {
                            break;
                        }
                    }
                }
                Err(_err) => {
                    // Skip parse errors, continue processing
                    continue;
                }
            }
        }
        // Record usage after stream ends
        record_stream_usage(&provider_id, &model_name, prompt_tokens, completion_tokens);
        // Channel will be closed automatically when tx is dropped
    });

    // Convert receiver to stream for axum SSE
    let sse_stream = ReceiverStream::new(rx);

    let mut headers = HeaderMap::new();
    headers.insert(
        "Cache-Control",
        "no-store, no-cache, must-revalidate".parse().unwrap(),
    );
    headers.insert("Pragma", "no-cache".parse().unwrap());
    headers.insert("Expires", "0".parse().unwrap());
    headers.insert(header::CONNECTION, "keep-alive".parse().unwrap());
    headers.insert("X-Accel-Buffering", "no".parse().unwrap());

    // Header passthrough: copy all upstream headers except content-length and transfer-encoding
    for (key, value) in upstream_headers {
        if let Some(header_key) = key {
            let key_name = header_key.as_str();
            if key_name != "content-length" && key_name != "transfer-encoding" {
                headers.insert(header_key, value);
            }
        }
    }

    let sse_response = crate::util::create_sse_stream(sse_stream);

    Ok((status_code, headers, sse_response).into_response())
}

/// Process a non-streaming request and return JSON response
pub async fn process_json_request(
    state: &AppState,
    ctx: RequestContext,
) -> Result<axum::response::Response> {
    let response = state
        .registry
        .http_client()
        .send_request(&ctx.url, ctx.body, ctx.headers)
        .await
        .map_err(LlmMapError::Http)?;
    let mut upstream_headers = response.headers().clone();
    let status_code = response.status();

    // Clone upstream headers, then strip problematic ones before sending to
    // the client (body will be re-serialized, and auth/proxy headers leak).
    util::remove_excluded_headers(&mut upstream_headers, None);

    if !status_code.is_success() {
        let body = response.bytes().await.map_err(LlmMapError::Http)?;

        return Ok((status_code, upstream_headers, body).into_response());
    }

    let response_json: Value = response
        .json()
        .await
        .map_err(|e| LlmMapError::Internal(e.into()))?;

    record_usage(
        &response_json,
        &ctx.provider_id,
        ctx.provider_endpoint.clone(),
        &ctx.model_name,
    );

    let res = ctx
        .executor
        .execute_response(response_json, status_code, &upstream_headers)
        .await?;

    let state_code = res.status.unwrap_or(status_code);

    if let Some(hs) = res.headers {
        upstream_headers.extend(hs);
    }

    Ok((state_code, upstream_headers, axum::Json(res.body)).into_response())
}

/// Helper function to handle both streaming and non-streaming requests
pub async fn handle_llm_request(
    state: &AppState,
    route: RouteEndpoint,
    headers: HeaderMap,
    body: Value,
    is_stream: bool,
) -> Result<axum::response::Response> {
    let ctx = execute_provider_request(state, route, headers, body).await?;

    let model_name = ctx.body["model"].as_str().unwrap_or("unknown");
    tracing::info!("[Upstream] POST {} | model: {}", ctx.url, model_name);

    if is_stream {
        process_stream_request(state, ctx).await
    } else {
        process_json_request(state, ctx).await
    }
}
