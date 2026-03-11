//! Generic route handler utilities for LLM endpoints

use crate::adapter::OnionExecutor;
use crate::error::{LlmMapError, Result};
use crate::model::{EndpointConfig, RequestTransform};
use crate::state::AppState;
use crate::util;
use axum::response::IntoResponse;
use axum::response::sse::Event;
use eventsource_stream::Eventsource;
use futures::stream::StreamExt;
use http::{HeaderMap, header};
use serde_json::{Value, json};
use tokio::sync::mpsc;

/// Context for processing provider responses
pub struct RequestContext {
    pub url: String,
    pub body: Value,
    pub headers: HeaderMap,
    pub executor: OnionExecutor,
}

/// Execute a provider request and return the transformed response
///
/// # Provider Configuration Handling
///
/// This function processes provider configuration in the following order:
/// 1. **Provider-level config**: Merges `provider.body` and `provider.headers` first
/// 2. **Adapter transformation**: Executes adapter chain which may modify body/headers
/// 3. **Model-specific config**: Applies model-specific body/headers from `provider.models` (if matched)
pub async fn execute_provider_request(
    state: &AppState,
    headers: &HeaderMap,
    mut body: Value,
    config: &EndpointConfig,
) -> Result<RequestContext> {
    let model_value = body
        .get("model")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| LlmMapError::Validation("Missing required field: model".to_string()))?;

    let (provider_id, model_name) = util::parse_model(&model_value)?;

    let provider = state
        .registry
        .get(provider_id)
        .ok_or_else(|| LlmMapError::Provider(format!("Provider '{}' not found", provider_id)))?;

    let url = config.build_url(&provider.base_url, model_name);

    // Update model field to use model_name only (without provider prefix)
    // Safety: We just verified model exists at line 107-111
    if let Some(model_field) = body.get_mut("model") {
        *model_field = Value::String(model_name.to_string());
    } else {
        return Err(LlmMapError::Internal(anyhow::anyhow!(
            "Model field disappeared after validation"
        )));
    }

    let mut final_headers = HeaderMap::new();

    let executor = state.registry.build_executor(provider_id)?;

    let RequestTransform {
        mut body,
        url: transform_url,
        headers: transform_headers,
    } = executor.execute_request(body, headers).await?;

    let final_url = transform_url.unwrap_or(url);

    // Merge provider-configured body fields into the request body
    if let Some(provider_body_fields) = provider.body.as_ref() {
        for body_entry in provider_body_fields {
            body[&body_entry.name] = body_entry.value.clone();
        }
    }

    if let Some(phs) = provider.headers.as_ref() {
        phs.iter().for_each(|header_entry| {
            if let Ok(header_name) = header_entry.name.parse::<http::header::HeaderName>()
                && let Ok(header_value) = header_entry.value.parse::<http::header::HeaderValue>()
            {
                final_headers.insert(header_name, header_value);
            }
        });
    }

    if let Some(hs) = transform_headers {
        crate::util::extend_overwrite(&mut final_headers, hs);
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

    Ok(RequestContext {
        url: final_url,
        body,
        headers: final_headers,
        executor,
    })
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
    } = ctx;

    headers.insert(header::ACCEPT, "text/event-stream".parse().unwrap());

    let response = state
        .registry
        .http_client()
        .send_request(&url, body, headers)
        .await
        .map_err(LlmMapError::Http)?;

    let status_code = response.status();
    let upstream_headers = response.headers().clone();

    // Check if status code indicates an error (e.g., 429 Too Many Requests)
    // If so, return error response instead of streaming
    if !status_code.is_success() {
        let body = response.bytes().await.map_err(LlmMapError::Http)?;
        return Ok((status_code, upstream_headers, body).into_response());
    }

    // Create channel for real-time streaming
    // Buffer size 32 is enough for real-time streaming without too much memory pressure
    let (tx, rx) = mpsc::channel::<std::result::Result<Event, axum::BoxError>>(32);

    // Spawn task to process upstream stream and send events via channel
    tokio::spawn(async move {
        let mut stream = response.bytes_stream().eventsource();

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
        // Channel will be closed automatically when tx is dropped
    });

    // Convert receiver to stream for axum SSE
    let sse_stream = futures::stream::unfold(rx, move |mut rx| async move {
        match rx.recv().await {
            Some(Ok(event)) => Some((Ok(event), rx)),
            Some(Err(e)) => Some((Err(e), rx)),
            None => None, // Channel closed, end stream
        }
    });

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
    let upstream_headers = response.headers().clone();
    let status_code = response.status();

    if !status_code.is_success() {
        let body = response.bytes().await.map_err(LlmMapError::Http)?;
        return Ok((status_code, upstream_headers, body).into_response());
    }

    // Build headers map, excluding content-length and transfer-encoding
    // since axum will recalculate these based on the transformed response
    let mut headers = HeaderMap::new();
    for (key, value) in upstream_headers {
        if let Some(header_key) = key {
            let key_name = header_key.as_str();
            if key_name != "content-length" && key_name != "transfer-encoding" {
                headers.insert(header_key, value);
            }
        }
    }
    let response_json: Value = response
        .json()
        .await
        .map_err(|e| LlmMapError::Internal(e.into()))?;

    let res = ctx
        .executor
        .execute_response(response_json, status_code, &headers)
        .await?;

    let state_code = res.status.unwrap_or(status_code);

    if let Some(hs) = res.headers {
        headers.extend(hs);
    }

    Ok((state_code, headers, axum::Json(res.body)).into_response())
}

/// Helper function to handle both streaming and non-streaming requests
pub async fn handle_llm_request(
    state: &AppState,
    headers: &HeaderMap,
    body: Value,
    config: &EndpointConfig,
    is_stream: bool,
) -> Result<axum::response::Response> {
    let ctx = execute_provider_request(state, headers, body, config).await?;

    if is_stream {
        process_stream_request(state, ctx).await
    } else {
        process_json_request(state, ctx).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_model_with_provider() {
        let result = util::parse_model("qwen-code@gpt-4");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ("qwen-code", "gpt-4"));
    }

    #[test]
    fn test_parse_model_without_provider() {
        assert!(util::parse_model("gpt-4").is_err());
    }

    #[test]
    fn test_join_url_paths() {
        assert_eq!(
            util::join_url_paths("https://api.example.com/", "/v1/chat"),
            "https://api.example.com/v1/chat"
        );
    }

    #[test]
    fn test_endpoint_config_build_url_with_model_placeholder() {
        let config = EndpointConfig::new("/v1/models/{model}:generateContent");
        assert_eq!(
            config.build_url("https://generativelanguage.googleapis.com", "gemini-pro"),
            "https://generativelanguage.googleapis.com/v1/models/gemini-pro:generateContent"
        );
    }
}
