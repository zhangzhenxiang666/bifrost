//! Generic route handler utilities for LLM endpoints

use crate::adapter::chain::OnionExecutor;
use crate::error::{LlmMapError, Result};
use crate::model::RequestTransform;
use crate::sse::IntoSseStream;
use crate::state::AppState;
use crate::types::AliasEntry;
use crate::util;
use axum::response::IntoResponse;
use axum::response::sse::Event;
use bifrost_shared::Endpoint;
use http::{HeaderMap, header};
use serde_json::Value;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;

use crate::routes::{RouteEndpoint, build_request_parts};
use bifrost_shared::types::{BodyTransformPolicy, PROTECTED_BODY_FIELDS};
use bifrost_shared::usage::{record_stream_usage, record_usage};
use std::collections::HashSet;

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

#[derive(Debug, Default)]
struct AliasExtras {
    headers: Option<Vec<crate::types::HeaderEntry>>,
    body: Option<Vec<crate::types::BodyEntry>>,
}

type ModelResolution = (String, String, Option<AliasExtras>);

fn resolve_model_target(
    body: &Value,
    registry: &crate::provider::registry::ProviderRegistry,
) -> Result<ModelResolution> {
    let model_value = body
        .get("model")
        .and_then(|v| v.as_str())
        .ok_or_else(|| LlmMapError::Validation("Missing required field: model".to_string()))?;

    let (model_target, alias_extras) = if model_value.contains('@') {
        (model_value.to_string(), None)
    } else {
        match registry.get_alias_entry(model_value) {
            Some(AliasEntry::Simple(target)) => (target.clone(), None),
            Some(AliasEntry::Complex(config)) => (
                config.target.clone(),
                Some(AliasExtras {
                    headers: config.headers.clone(),
                    body: config.body.clone(),
                }),
            ),
            None => {
                return Err(LlmMapError::Validation(format!(
                    "Unknown model '{}'. Expected format: provider@model (e.g., 'openai@gpt-4o')",
                    model_value
                )));
            }
        }
    };

    let (provider_id, model_name) = util::parse_model(&model_target)?;
    Ok((
        provider_id.to_string(),
        model_name.to_string(),
        alias_extras,
    ))
}

/// Check if a body/header entry should be applied based on its condition
fn should_apply_entry(condition: &Option<String>, route: &RouteEndpoint) -> bool {
    match condition {
        None => true, // No condition, apply to all endpoints
        Some(cond) => {
            // Normalize: replace hyphens with underscores for consistent matching
            let cond_normalized = cond.to_lowercase().replace('-', "_");
            let matches = match cond_normalized.as_str() {
                "openai_chat" | "openai-chat" => matches!(route, RouteEndpoint::OpenAIChat),
                "openai_responses" | "openai-responses" => {
                    matches!(route, RouteEndpoint::OpenAIResponses)
                }
                "anthropic" => matches!(route, RouteEndpoint::AnthropicMessages),
                _ => {
                    tracing::warn!(
                        condition = %cond,
                        "Unknown condition value, ignoring entry"
                    );
                    false
                }
            };
            if !matches {
                tracing::debug!(
                    condition = %cond,
                    route = ?route,
                    "Skipping entry due to condition mismatch"
                );
            }
            matches
        }
    }
}

fn merge_provider_config_into_request(
    body: &mut Value,
    headers: &mut HeaderMap,
    provider: &crate::types::ProviderConfig,
    alias_extras: Option<AliasExtras>,
    model_name: &str,
    route: &RouteEndpoint,
) -> Result<()> {
    if let Some(extras) = alias_extras.as_ref().and_then(|e| e.body.as_ref()) {
        for body_entry in extras {
            if should_apply_entry(&body_entry.condition, route) {
                body[&body_entry.name] = body_entry.value.clone();
            }
        }
    }

    if let Some(extras) = alias_extras.as_ref().and_then(|e| e.headers.as_ref()) {
        for header_entry in extras {
            if should_apply_entry(&header_entry.condition, route)
                && let Ok(header_name) = header_entry.name.parse::<http::header::HeaderName>()
                && let Ok(header_value) = header_entry.value.parse::<http::header::HeaderValue>()
            {
                headers.insert(header_name, header_value);
            }
        }
    }

    if let Some(provider_body_fields) = provider.body.as_ref() {
        for body_entry in provider_body_fields {
            if !should_apply_entry(&body_entry.condition, route) {
                continue;
            }
            if PROTECTED_BODY_FIELDS.contains(&body_entry.name.as_str()) {
                tracing::warn!(
                    field = %body_entry.name,
                    "Ignoring protected field in provider body config"
                );
                continue;
            }
            body[&body_entry.name] = body_entry.value.clone();
        }
    }

    if let Some(provider_headers) = provider.headers.as_ref() {
        for header_entry in provider_headers {
            if !should_apply_entry(&header_entry.condition, route) {
                continue;
            }
            if let Ok(header_name) = header_entry.name.parse::<http::header::HeaderName>()
                && let Ok(header_value) = header_entry.value.parse::<http::header::HeaderValue>()
            {
                headers.insert(header_name, header_value);
            }
        }
    }

    if let Some(models_config) = provider.models.as_ref()
        && let Some(model_cfg) = models_config.iter().find(|m| m.name == model_name)
    {
        if let Some(model_body_fields) = model_cfg.body.as_ref() {
            for body_entry in model_body_fields {
                if !should_apply_entry(&body_entry.condition, route) {
                    continue;
                }
                if PROTECTED_BODY_FIELDS.contains(&body_entry.name.as_str()) {
                    tracing::warn!(
                        model = %model_name,
                        field = %body_entry.name,
                        "Ignoring protected field in model body config"
                    );
                    continue;
                }
                body[&body_entry.name] = body_entry.value.clone();
            }
        }
        if let Some(model_headers) = model_cfg.headers.as_ref() {
            for header_entry in model_headers {
                if !should_apply_entry(&header_entry.condition, route) {
                    continue;
                }
                if let Ok(header_name) = header_entry.name.parse::<http::header::HeaderName>()
                    && let Ok(header_value) =
                        header_entry.value.parse::<http::header::HeaderValue>()
                {
                    headers.insert(header_name, header_value);
                }
            }
        }
    }

    Ok(())
}

pub async fn execute_provider_request(
    state: &AppState,
    route: RouteEndpoint,
    mut headers: HeaderMap,
    mut body: Value,
) -> Result<RequestContext> {
    let (provider_id, model_name, alias_extras) = resolve_model_target(&body, &state.registry)?;

    let provider = state
        .registry
        .get(&provider_id)
        .ok_or_else(|| LlmMapError::Provider(format!("Provider '{}' not found", provider_id)))?;

    *body.get_mut("model").unwrap() = Value::String(model_name.clone());

    let mut final_headers = HeaderMap::new();

    let executor = state.registry.build_executor(&provider_id, &route)?;

    let RequestTransform { mut body } = executor.execute_request(body).await?;

    if let Some(policy) = provider.body_policy.as_ref() {
        apply_body_policy_to_value(&mut body, policy);
    }

    if provider.extend {
        util::remove_excluded_headers(&mut headers, provider.exclude_headers.as_deref());
        util::extend_overwrite(&mut final_headers, headers);
    }

    let span = tracing::info_span!("merge_config", provider_id = %provider_id);
    let _enter = span.enter();

    merge_provider_config_into_request(
        &mut body,
        &mut final_headers,
        provider,
        alias_extras,
        &model_name,
        &route,
    )?;

    let (url, auth_headers) = build_request_parts(provider);
    util::extend_overwrite(&mut final_headers, auth_headers);

    Ok(RequestContext {
        url,
        body,
        headers: final_headers,
        executor,
        provider_endpoint: provider.endpoint.clone(),
        provider_id,
        model_name,
    })
}

fn apply_body_policy_to_value(body: &mut Value, policy: &BodyTransformPolicy) {
    let Some(map) = body.as_object_mut() else {
        return;
    };

    match policy {
        BodyTransformPolicy::PreserveUnknown => {}
        BodyTransformPolicy::DropUnknown => {
            map.retain(|k, _| PROTECTED_BODY_FIELDS.contains(&k.as_str()));
        }
        BodyTransformPolicy::Allowlist(fields) => {
            let allowed: HashSet<_> = PROTECTED_BODY_FIELDS
                .iter()
                .copied()
                .chain(fields.iter().map(|s| s.as_str()))
                .collect();
            map.retain(|k, _| allowed.contains(k.as_str()));
        }
        BodyTransformPolicy::Blocklist(fields) => {
            let blocked: HashSet<_> = fields.iter().map(|s| s.as_str()).collect();
            map.retain(|k, _| !blocked.contains(k.as_str()));
        }
    }
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
    let (tx, rx) = mpsc::channel::<std::result::Result<Event, axum::BoxError>>(256);

    let span = tracing::Span::current();

    // Spawn task to process upstream stream and send events via channel
    tokio::spawn(async move {
        let _guard = span.enter();
        let mut stream = Box::pin(
            response
                .bytes_stream()
                .into_sse_stream()
                .timeout(Duration::from_secs(90)),
        );
        let mut prompt_tokens: u32 = 0;
        let mut completion_tokens: u32 = 0;
        let mut consecutive_errors: u32 = 0;
        const MAX_CONSECUTIVE_ERRORS: u32 = 10;

        'stream: while let Some(result) = stream.next().await {
            match result {
                Ok(Ok(event)) => {
                    consecutive_errors = 0;

                    if event.data.starts_with("[DONE]") {
                        tracing::debug!(msg = "Received [DONE] sentinel, ending stream");
                        break;
                    }

                    let chunk: Value = match serde_json::from_str(&event.data) {
                        Ok(data) => data,
                        Err(error) => {
                            tracing::warn!(msg = "Failed to parse SSE event data", error = %error, data = %event.data);
                            continue;
                        }
                    };

                    try_extract_usage(
                        &chunk,
                        &event.event,
                        &provider_endpoint,
                        &mut prompt_tokens,
                        &mut completion_tokens,
                    );

                    let transform = executor.execute_stream_chunk(chunk, event.event).await;

                    match transform {
                        Ok(transform) => {
                            for (data, event_name) in transform.events {
                                let data_str = serde_json::to_string(&data)
                                    .unwrap_or_else(|_| "{}".to_string());
                                let mut sse_event = Event::default();
                                if let Some(name) = event_name {
                                    sse_event = sse_event.event(name);
                                }
                                sse_event = sse_event.data(data_str);
                                match tx.send(Ok(sse_event)).await {
                                    Ok(()) => {}
                                    Err(error) => {
                                        tracing::warn!(msg = "SSE client disconnected, stopping stream", error = %error);
                                        break 'stream;
                                    }
                                }
                            }
                        }
                        Err(error) => {
                            tracing::warn!(msg = "Error executing stream chunk", error = %error);
                        }
                    }
                }
                Ok(Err(err)) => {
                    consecutive_errors += 1;
                    tracing::warn!(
                        msg = %err,
                        r#type = "sse-parser",
                        consecutive_errors = consecutive_errors,
                        max_consecutive_errors = MAX_CONSECUTIVE_ERRORS,
                    );
                    if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                        tracing::error!(
                            msg = "Too many consecutive SSE errors, stopping stream",
                            consecutive_errors = consecutive_errors,
                        );
                        break;
                    }
                    continue;
                }
                Err(_elapsed) => {
                    tracing::warn!("SSE stream timed out after 90s");
                    break;
                }
            }
        }
        // Record usage after stream ends
        record_stream_usage(&provider_id, &model_name, prompt_tokens, completion_tokens);
        // Channel will be closed automatically when tx is dropped
        drop(tx);
        tracing::debug!(msg = "SSE stream sender dropped, channel closed");
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
    tracing::info!(url = %ctx.url, model = %model_name, r#type = "handler");

    if is_stream {
        process_stream_request(state, ctx).await
    } else {
        process_json_request(state, ctx).await
    }
}
