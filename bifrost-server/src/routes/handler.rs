//! Generic route handler utilities for LLM endpoints

use crate::adapter::chain::OnionExecutor;
use crate::error::{LlmMapError, Result};
use crate::model::RequestTransform;
use crate::provider::registry::ProviderDeployment;
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

use crate::routes::{RouteEndpoint, build_auth_headers, build_request_url};
use bifrost_shared::types::{BodyTransformPolicy, PROTECTED_BODY_FIELDS};
use bifrost_shared::usage::{record_stream_usage, record_usage};
use std::collections::HashSet;

const DEPLOYMENT_OVERRIDE_HEADER: &str = "x-bifrost-deployment";
const PROVIDER_RESPONSE_HEADER: &str = "x-bifrost-provider";
const DEPLOYMENT_RESPONSE_HEADER: &str = "x-bifrost-deployment";
const FALLBACK_COUNT_RESPONSE_HEADER: &str = "x-bifrost-fallback-count";

/// Context for processing provider responses
pub struct RequestContext {
    pub body: Value,
    pub headers: HeaderMap,
    pub executor: OnionExecutor,
    /// Upstream provider endpoint type (openai or anthropic)
    pub provider_endpoint: Endpoint,
    /// Provider ID from config
    pub provider_id: String,
    /// Model name being called
    pub model_name: String,
    /// Ordered deployments to try for this request.
    pub deployment_plan: Vec<ProviderDeployment>,
}

#[derive(Debug, Default)]
struct AliasExtras {
    deployment: Option<String>,
    headers: Option<Vec<crate::types::HeaderEntry>>,
    body: Option<Vec<crate::types::BodyEntry>>,
}

type ModelResolution = (String, String, Option<String>, Option<AliasExtras>);

struct UpstreamAttempt {
    response: reqwest::Response,
    deployment_id: String,
    fallback_count: usize,
}

fn split_model_deployment(model_value: &str) -> (&str, Option<String>) {
    if let Some((model, deployment)) = model_value.rsplit_once('#')
        && !model.is_empty()
        && !deployment.trim().is_empty()
    {
        return (model, Some(deployment.trim().to_string()));
    }

    (model_value, None)
}

fn resolve_model_target(
    body: &Value,
    registry: &crate::provider::registry::ProviderRegistry,
) -> Result<ModelResolution> {
    let model_value = body
        .get("model")
        .and_then(|v| v.as_str())
        .ok_or_else(|| LlmMapError::Validation("Missing required field: model".to_string()))?;

    let (model_selector, model_deployment) = split_model_deployment(model_value);

    let (model_target, alias_extras) = if model_selector.contains('@') {
        (model_selector.to_string(), None)
    } else {
        match registry.get_alias_entry(model_selector) {
            Some(AliasEntry::Simple(target)) => (target.clone(), None),
            Some(AliasEntry::Complex(config)) => (
                config.target.clone(),
                Some(AliasExtras {
                    deployment: config.deployment.clone(),
                    headers: config.headers.clone(),
                    body: config.body.clone(),
                }),
            ),
            None => {
                return Err(LlmMapError::Validation(format!(
                    "Unknown model '{}'. Expected format: provider@model (e.g., 'openai@gpt-4o')",
                    model_selector
                )));
            }
        }
    };

    let (provider_id, model_name) = util::parse_model(&model_target)?;
    Ok((
        provider_id.to_string(),
        model_name.to_string(),
        model_deployment,
        alias_extras,
    ))
}

fn deployment_override_from_headers(headers: &HeaderMap) -> Result<Option<String>> {
    match headers.get(DEPLOYMENT_OVERRIDE_HEADER) {
        Some(value) => Ok(Some(
            value
                .to_str()
                .map_err(|_| {
                    LlmMapError::Validation(format!(
                        "Header '{}' must be valid ASCII",
                        DEPLOYMENT_OVERRIDE_HEADER
                    ))
                })?
                .trim()
                .to_string(),
        )),
        None => Ok(None),
    }
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
    let header_deployment = deployment_override_from_headers(&headers)?;
    if let Some(deployment) = &header_deployment
        && deployment.is_empty()
    {
        return Err(LlmMapError::Validation(format!(
            "Header '{}' cannot be empty",
            DEPLOYMENT_OVERRIDE_HEADER
        )));
    }

    let (provider_id, model_name, model_deployment, alias_extras) =
        resolve_model_target(&body, &state.registry)?;

    let provider = state
        .registry
        .get(&provider_id)
        .ok_or_else(|| LlmMapError::Provider(format!("Provider '{}' not found", provider_id)))?;

    let alias_deployment = alias_extras
        .as_ref()
        .and_then(|extras| extras.deployment.clone());
    let deployment_preference = header_deployment
        .as_deref()
        .or(model_deployment.as_deref())
        .or(alias_deployment.as_deref());
    let deployment_plan = state
        .registry
        .deployment_plan(&provider_id, deployment_preference)?;

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

    Ok(RequestContext {
        body,
        headers: final_headers,
        executor,
        provider_endpoint: provider.endpoint.clone(),
        provider_id,
        model_name,
        deployment_plan,
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
    cached_tokens: &mut Option<u32>,
    cache_creation_tokens: &mut Option<u32>,
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
                let cached = usage
                    .get("prompt_tokens_details")
                    .and_then(|d| d.get("cached_tokens"))
                    .or_else(|| usage.get("prompt_cache_hit_tokens"))
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32)
                    .filter(|&v| v > 0);
                if cached.is_some() {
                    *cached_tokens = cached;
                }
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
                let cache_read = usage
                    .get("cache_read_input_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
                let cache_creation = usage
                    .get("cache_creation_input_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
                *prompt_tokens += cache_read + cache_creation;
                if cache_read > 0 {
                    *cached_tokens = Some(cache_read);
                }
                if cache_creation > 0 {
                    *cache_creation_tokens = Some(cache_creation);
                }
            } else if event == "message_delta"
                && let Some(usage) = chunk.get("usage").and_then(|u| u.as_object())
            {
                *completion_tokens = usage
                    .get("output_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
                // Some providers send input_tokens in message_delta instead of message_start.
                if *prompt_tokens == 0
                    && let Some(delta_input) = usage
                        .get("input_tokens")
                        .and_then(|v| v.as_u64())
                        .map(|v| v as u32)
                {
                    *prompt_tokens = delta_input;
                }
                let cache_read = usage
                    .get("cache_read_input_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
                let cache_creation = usage
                    .get("cache_creation_input_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
                if cache_read > 0 && cached_tokens.is_none() {
                    *cached_tokens = Some(cache_read);
                    *prompt_tokens += cache_read;
                }
                if cache_creation > 0 && cache_creation_tokens.is_none() {
                    *cache_creation_tokens = Some(cache_creation);
                    *prompt_tokens += cache_creation;
                }
            }
        }
    }
}

fn insert_bifrost_response_headers(
    headers: &mut HeaderMap,
    provider_id: &str,
    deployment_id: &str,
    fallback_count: usize,
) {
    if let Ok(value) = provider_id.parse() {
        headers.insert(PROVIDER_RESPONSE_HEADER, value);
    }
    if let Ok(value) = deployment_id.parse() {
        headers.insert(DEPLOYMENT_RESPONSE_HEADER, value);
    }
    if let Ok(value) = fallback_count.to_string().parse() {
        headers.insert(FALLBACK_COUNT_RESPONSE_HEADER, value);
    }
}

async fn send_upstream_request(
    state: &AppState,
    body: &Value,
    base_headers: &HeaderMap,
    provider_endpoint: &Endpoint,
    provider_id: &str,
    deployment_plan: &[ProviderDeployment],
) -> Result<UpstreamAttempt> {
    let Some((last_deployment, deployments_before_last)) = deployment_plan.split_last() else {
        return Err(LlmMapError::Provider(format!(
            "Provider '{}' has no deployments to try",
            provider_id
        )));
    };

    for (attempt_index, deployment) in deployments_before_last.iter().enumerate() {
        let mut headers = base_headers.clone();
        util::extend_overwrite(
            &mut headers,
            build_auth_headers(provider_endpoint, &deployment.api_key),
        );
        let url = build_request_url(&deployment.base_url, provider_endpoint);

        match state
            .registry
            .http_client()
            .send_request(&url, body.clone(), headers)
            .await
        {
            Ok(response) => {
                let status_code = response.status().as_u16();
                if state
                    .registry
                    .http_client()
                    .is_retryable_status_code(status_code)
                {
                    state
                        .registry
                        .record_deployment_failure(provider_id, &deployment.id);
                    tracing::warn!(
                        provider_id = %provider_id,
                        deployment_id = %deployment.id,
                        status_code,
                        "Upstream request failed with retryable status, trying next deployment"
                    );
                    let _ = response.bytes().await;
                    continue;
                }

                state
                    .registry
                    .record_deployment_success(provider_id, &deployment.id);
                return Ok(UpstreamAttempt {
                    response,
                    deployment_id: deployment.id.clone(),
                    fallback_count: attempt_index,
                });
            }
            Err(error) if crate::provider::client::HttpClient::is_retryable_error(&error) => {
                state
                    .registry
                    .record_deployment_failure(provider_id, &deployment.id);
                tracing::warn!(
                    provider_id = %provider_id,
                    deployment_id = %deployment.id,
                    error = %error,
                    "Upstream request failed with retryable error, trying next deployment"
                );
                continue;
            }
            Err(error) => return Err(LlmMapError::Http(error)),
        }
    }

    let mut headers = base_headers.clone();
    util::extend_overwrite(
        &mut headers,
        build_auth_headers(provider_endpoint, &last_deployment.api_key),
    );
    let url = build_request_url(&last_deployment.base_url, provider_endpoint);
    let response = state
        .registry
        .http_client()
        .send_request(&url, body.clone(), headers)
        .await
        .inspect_err(|error| {
            if crate::provider::client::HttpClient::is_retryable_error(error) {
                state
                    .registry
                    .record_deployment_failure(provider_id, &last_deployment.id);
            }
        })
        .map_err(LlmMapError::Http)?;

    if state
        .registry
        .http_client()
        .is_retryable_status_code(response.status().as_u16())
    {
        state
            .registry
            .record_deployment_failure(provider_id, &last_deployment.id);
    } else {
        state
            .registry
            .record_deployment_success(provider_id, &last_deployment.id);
    }

    Ok(UpstreamAttempt {
        response,
        deployment_id: last_deployment.id.clone(),
        fallback_count: deployments_before_last.len(),
    })
}

/// Process a streaming request and return SSE response
pub async fn process_stream_request(
    state: &AppState,
    ctx: RequestContext,
) -> Result<axum::response::Response> {
    let RequestContext {
        body,
        mut headers,
        executor,
        provider_endpoint,
        provider_id,
        model_name,
        deployment_plan,
    } = ctx;

    headers.insert(header::ACCEPT, "text/event-stream".parse().unwrap());

    let upstream_attempt = send_upstream_request(
        state,
        &body,
        &headers,
        &provider_endpoint,
        &provider_id,
        &deployment_plan,
    )
    .await?;
    let response = upstream_attempt.response;
    let deployment_id = upstream_attempt.deployment_id;
    let fallback_count = upstream_attempt.fallback_count;

    let status_code = response.status();
    let mut upstream_headers = response.headers().clone();

    // Strip headers that conflict with axum's auto-generated response headers
    util::remove_excluded_headers(&mut upstream_headers, None);
    insert_bifrost_response_headers(
        &mut upstream_headers,
        &provider_id,
        &deployment_id,
        fallback_count,
    );

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
        let mut cached_tokens: Option<u32> = None;
        let mut cache_creation_tokens: Option<u32> = None;
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
                        &mut cached_tokens,
                        &mut cache_creation_tokens,
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
        record_stream_usage(
            &provider_id,
            &model_name,
            Some(&deployment_id),
            prompt_tokens,
            completion_tokens,
            cached_tokens,
        );
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
    let upstream_attempt = send_upstream_request(
        state,
        &ctx.body,
        &ctx.headers,
        &ctx.provider_endpoint,
        &ctx.provider_id,
        &ctx.deployment_plan,
    )
    .await?;
    let response = upstream_attempt.response;
    let deployment_id = upstream_attempt.deployment_id;
    let fallback_count = upstream_attempt.fallback_count;
    let mut upstream_headers = response.headers().clone();
    let status_code = response.status();

    // Clone upstream headers, then strip problematic ones before sending to
    // the client (body will be re-serialized, and auth/proxy headers leak).
    util::remove_excluded_headers(&mut upstream_headers, None);
    insert_bifrost_response_headers(
        &mut upstream_headers,
        &ctx.provider_id,
        &deployment_id,
        fallback_count,
    );

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
        Some(&deployment_id),
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
    tracing::info!(
        provider_id = %ctx.provider_id,
        model = %model_name,
        r#type = "handler"
    );

    if is_stream {
        process_stream_request(state, ctx).await
    } else {
        process_json_request(state, ctx).await
    }
}

#[cfg(test)]
mod tests {
    use super::split_model_deployment;

    #[test]
    fn split_model_deployment_supports_hash_suffix() {
        assert_eq!(
            split_model_deployment("provider@gpt-4o#work"),
            ("provider@gpt-4o", Some("work".to_string()))
        );
        assert_eq!(
            split_model_deployment("sonnet#backup"),
            ("sonnet", Some("backup".to_string()))
        );
    }
}
