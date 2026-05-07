//! Stream processor for converting Chat API streaming chunks to Responses API SSE events

use super::state::ChatToResponsesStreamState;
use crate::adapter::converter::openai_responses::NamespaceMappings;
use crate::error::LlmMapError;
use crate::model::StreamChunkTransform;
use serde_json::{Value, json};
use std::cell::UnsafeCell;

/// Stream processor for converting Chat API stream chunks to Responses API SSE events.
///
/// # Safety Invariant
///
/// `stream_state` is wrapped in `UnsafeCell` to allow interior mutability without
/// any locking overhead. This is sound under the following architectural guarantee:
///
/// - One [`ChatToResponsesStreamProcessor`] instance is created per request.
/// - All method calls on that instance occur sequentially; no two call-sites
///   ever execute concurrently on the same instance.
///
/// Consequently there is never more than one live `&` or `&mut` reference to
/// `stream_state` at a time, which is the only condition `UnsafeCell` requires.
/// Violating this invariant is undefined behavior.
pub struct ChatToResponsesStreamProcessor {
    stream_state: UnsafeCell<ChatToResponsesStreamState>,
}

// SAFETY: The processor is never shared across threads concurrently.
// Each request owns its own instance and drives it from a single async task.
unsafe impl Sync for ChatToResponsesStreamProcessor {}

// SAFETY: Ownership may cross thread boundaries at Tokio await points, but only
// one thread holds the processor at any given moment, and `ChatToResponsesStreamState`
// contains no thread-local state.
unsafe impl Send for ChatToResponsesStreamProcessor {}

impl Default for ChatToResponsesStreamProcessor {
    fn default() -> Self {
        Self::new()
    }
}

fn build_response_obj(
    id: &str,
    model: &str,
    created_at: u64,
    output: Value,
    status: &str,
) -> Value {
    json!({
        "object": "response",
        "id": id,
        "created_at": created_at,
        "model": model,
        "output": output,
        "status": status,
        "parallel_tool_calls": true,
        "tools": []
    })
}

// ── Event template helpers ──

fn sse_event_base(event_type: &str, seq: u64) -> serde_json::Map<String, Value> {
    let mut map = serde_json::Map::with_capacity(4);
    map.insert("type".into(), Value::String(event_type.into()));
    map.insert("sequence_number".into(), Value::Number(seq.into()));
    map
}

fn build_output_item_added_event(
    seq: u64,
    output_index: u64,
    item: Value,
) -> (Value, Option<String>) {
    let mut map = sse_event_base("response.output_item.added", seq);
    map.insert("output_index".into(), Value::Number(output_index.into()));
    map.insert("item".into(), item);
    (
        Value::Object(map),
        Some("response.output_item.added".into()),
    )
}

fn build_output_item_done_event(
    seq: u64,
    output_index: u64,
    item: Value,
) -> (Value, Option<String>) {
    let mut map = sse_event_base("response.output_item.done", seq);
    map.insert("output_index".into(), Value::Number(output_index.into()));
    map.insert("item".into(), item);
    (Value::Object(map), Some("response.output_item.done".into()))
}

fn build_content_part_added_event(
    seq: u64,
    output_index: u64,
    item_id: &str,
    content_index: u64,
    part: Value,
) -> (Value, Option<String>) {
    let mut map = sse_event_base("response.content_part.added", seq);
    map.insert("output_index".into(), Value::Number(output_index.into()));
    map.insert("item_id".into(), Value::String(item_id.into()));
    map.insert("content_index".into(), Value::Number(content_index.into()));
    map.insert("part".into(), part);
    (
        Value::Object(map),
        Some("response.content_part.added".into()),
    )
}

fn build_content_part_done_event(
    seq: u64,
    output_index: u64,
    item_id: &str,
    content_index: u64,
    part: Value,
) -> (Value, Option<String>) {
    let mut map = sse_event_base("response.content_part.done", seq);
    map.insert("output_index".into(), Value::Number(output_index.into()));
    map.insert("item_id".into(), Value::String(item_id.into()));
    map.insert("content_index".into(), Value::Number(content_index.into()));
    map.insert("part".into(), part);
    (
        Value::Object(map),
        Some("response.content_part.done".into()),
    )
}

fn build_delta_event(
    event_type: &str,
    seq: u64,
    output_index: u64,
    item_id: &str,
    delta: &str,
    extra: Vec<(&str, Value)>,
) -> (Value, Option<String>) {
    let mut map = sse_event_base(event_type, seq);
    map.insert("output_index".into(), Value::Number(output_index.into()));
    map.insert("item_id".into(), Value::String(item_id.into()));
    map.insert("delta".into(), Value::String(delta.into()));
    for (k, v) in extra {
        map.insert(k.into(), v);
    }
    (Value::Object(map), Some(event_type.into()))
}

fn build_function_call_arguments_done_event(
    seq: u64,
    output_index: u64,
    item_id: &str,
    arguments: &str,
) -> (Value, Option<String>) {
    let mut map = sse_event_base("response.function_call_arguments.done", seq);
    map.insert("output_index".into(), Value::Number(output_index.into()));
    map.insert("item_id".into(), Value::String(item_id.into()));
    map.insert("arguments".into(), Value::String(arguments.into()));
    (
        Value::Object(map),
        Some("response.function_call_arguments.done".into()),
    )
}

impl ChatToResponsesStreamProcessor {
    pub fn new() -> Self {
        Self {
            stream_state: UnsafeCell::new(ChatToResponsesStreamState::new()),
        }
    }

    /// Set namespace mappings for reverse-lookup of prefixed tool call names.
    pub fn set_namespace_mappings(&self, mappings: NamespaceMappings) {
        self.state_mut().set_namespace_mappings(mappings);
    }

    /// Get current namespace mappings.
    pub fn namespace_mappings(&self) -> NamespaceMappings {
        self.state().namespace_mappings().clone()
    }

    /// Immutable access — use for all reads.
    ///
    /// SAFETY: No `&mut` obtained via `state_mut` may be alive simultaneously.
    /// Upheld because every `state_mut()` call is a single-expression statement
    /// whose returned reference expires before the next statement executes.
    #[inline(always)]
    fn state(&self) -> &ChatToResponsesStreamState {
        unsafe { &*self.stream_state.get() }
    }

    /// Mutable access — call only for the single mutation, then let the
    /// reference expire immediately at the semicolon / end of the expression.
    ///
    /// SAFETY: Same guarantee as above; never store the returned reference
    /// across any other `state()` / `state_mut()` call.
    #[inline(always)]
    #[allow(clippy::mut_from_ref)]
    fn state_mut(&self) -> &mut ChatToResponsesStreamState {
        unsafe { &mut *self.stream_state.get() }
    }

    /// Convert a single Chat API streaming chunk to Responses API SSE events.
    pub fn chat_stream_to_responses_stream(
        &self,
        chunk: Value,
    ) -> Result<StreamChunkTransform, LlmMapError> {
        let obj = chunk
            .as_object()
            .ok_or_else(|| LlmMapError::Validation("Invalid chunk format".into()))?;

        let choices = obj.get("choices").and_then(|v| v.as_array());
        let Some(choice) = choices.and_then(|c| c.first()).and_then(|v| v.as_object()) else {
            return Ok(StreamChunkTransform::new_empty());
        };

        let delta = choice.get("delta").and_then(|v| v.as_object());
        let finish_reason = choice.get("finish_reason").and_then(|v| v.as_str());
        let usage = obj.get("usage").and_then(|v| v.as_object());

        if !self.state().has_created_sent() {
            let mut events = self.generate_initial_events(obj, delta, usage)?;
            if let Some(reason) = finish_reason {
                events.extend(self.generate_finishing_events(Some(reason), usage)?.events);
            }
            Ok(StreamChunkTransform::new_multi(events))
        } else {
            Ok(StreamChunkTransform::new_multi(
                self.generate_content_events_from_delta(delta, finish_reason, usage)?,
            ))
        }
    }

    fn generate_initial_events(
        &self,
        obj: &serde_json::Map<String, Value>,
        delta: Option<&serde_json::Map<String, Value>>,
        usage: Option<&serde_json::Map<String, Value>>,
    ) -> Result<Vec<(Value, Option<String>)>, LlmMapError> {
        let mut events = Vec::with_capacity(6);

        let id = obj.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let model = obj.get("model").and_then(|v| v.as_str()).unwrap_or("");
        let input_tokens = usage
            .and_then(|u| u.get("prompt_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        let output_tokens = usage
            .and_then(|u| u.get("completion_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(1) as u32;
        let reasoning_tokens = usage
            .and_then(|u| u.get("completion_tokens_details"))
            .and_then(|d| d.get("reasoning_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;

        // Extract created_at timestamp.
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let response_obj = build_response_obj(id, model, created_at, json!([]), "in_progress");

        // response.created event (sequence_number: 0)
        let seq_0 = self.state_mut().next_sequence();
        events.push((
            json!({
                "type": "response.created",
                "sequence_number": seq_0,
                "response": response_obj,
                "output_index": 0
            }),
            Some("response.created".to_string()),
        ));

        // response.in_progress event (sequence_number: 1)
        let seq_1 = self.state_mut().next_sequence();
        events.push((
            json!({
                "type": "response.in_progress",
                "sequence_number": seq_1,
                "response": response_obj,
                "output_index": 0
            }),
            Some("response.in_progress".to_string()),
        ));

        // Record first chunk metadata + usage in one &mut round-trip.
        {
            let s = self.state_mut();
            s.set_first_chunk_meta(id.to_string(), model.to_string());
            s.set_created_sent();
            s.set_in_progress_sent();
            s.set_usage(input_tokens, output_tokens, reasoning_tokens);
        }

        events.extend(self.generate_content_events_from_delta(delta, None, usage)?);

        Ok(events)
    }

    fn generate_content_events_from_delta(
        &self,
        delta: Option<&serde_json::Map<String, Value>>,
        finish_reason: Option<&str>,
        usage: Option<&serde_json::Map<String, Value>>,
    ) -> Result<Vec<(Value, Option<String>)>, LlmMapError> {
        let mut events = Vec::with_capacity(2);

        let reasoning = delta
            .and_then(|d| d.get("reasoning_content"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty());

        let text = delta
            .and_then(|d| d.get("content"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty());

        // ── Reasoning completion ──
        if self.state().has_reasoning_started()
            && !self.state().has_reasoning_done()
            && reasoning.is_none()
        {
            events.extend(self.generate_reasoning_done_events())
        }

        // --- text completion ──
        if self.state().has_text_started() && !self.state().has_text_done() && text.is_none() {
            events.extend(self.genrate_text_done_events())
        }

        // ── Process reasoning ──
        if let Some(reasoning_text) = reasoning {
            if !self.state().has_reasoning_started() {
                self.state_mut().set_reasoning_item_id(
                    crate::adapter::converter::openai_responses::create_item_id("item"),
                );
                let item_id = self.state().reasoning_item_id().unwrap();
                let output_index = self.state().item_counter();

                let seq = self.state_mut().next_sequence();
                events.push(build_output_item_added_event(
                    seq,
                    output_index,
                    json!({
                        "id": &item_id,
                        "type": "reasoning",
                        "summary": [],
                        "status": "in_progress"
                    }),
                ));

                let seq = self.state_mut().next_sequence();
                events.push((
                    json!({
                        "type": "response.reasoning_summary_part.added",
                        "sequence_number": seq,
                        "output_index": output_index,
                        "item_id": &item_id,
                        "part": { "type": "summary_text" },
                        "summary_index": 0
                    }),
                    Some("response.reasoning_summary_part.added".to_string()),
                ));

                self.state_mut().set_reasoning_started();
            }

            self.state_mut().push_reasoning(reasoning_text);

            let output_index = self.state().item_counter();
            let item_id = self.state().reasoning_item_id().unwrap();
            let seq = self.state_mut().next_sequence();
            events.push(build_delta_event(
                "response.reasoning_summary_text.delta",
                seq,
                output_index,
                item_id,
                reasoning_text,
                vec![("summary_index", Value::Number(0u64.into()))],
            ));
        }

        // ── Process text ──
        if let Some(text_content) = text {
            if !self.state().has_text_started() {
                self.state_mut().set_item_id(
                    crate::adapter::converter::openai_responses::create_item_id("item"),
                );
                let item_id = self.state().item_id().unwrap();

                // 如果模型已经发送了reasoning， 那么需要为output_index + 1
                let output_index = if self.state().has_reasoning_started() {
                    self.state_mut().next_output_index();
                    self.state().item_counter()
                } else {
                    self.state().item_counter()
                };

                let seq = self.state_mut().next_sequence();
                events.push(build_output_item_added_event(
                    seq,
                    output_index,
                    json!({
                        "id": &item_id,
                        "type": "message",
                        "role": "assistant",
                        "content": [],
                        "status": "in_progress"
                    }),
                ));

                let seq = self.state_mut().next_sequence();
                events.push(build_content_part_added_event(
                    seq,
                    output_index,
                    item_id,
                    0,
                    json!({ "type": "output_text", "text": "", "annotations": [], "logprobs": null }),
                ));
                self.state_mut().set_text_started();
            }

            self.state_mut().push_text(text_content);

            // TODO: item_id是option, 按照逻辑来说可以unwrap
            let item_id = self.state_mut().item_id().unwrap();
            let output_index = self.state().item_counter();
            let seq = self.state_mut().next_sequence();
            events.push(build_delta_event(
                "response.output_text.delta",
                seq,
                output_index,
                item_id,
                text_content,
                vec![("content_index", Value::Number(0u64.into()))],
            ));
        }

        // ── Process tool_calls ──
        if let Some(tool_calls) = delta
            .and_then(|d| d.get("tool_calls"))
            .and_then(|v| v.as_array())
        {
            for tool_call_value in tool_calls {
                let tool_call = tool_call_value
                    .as_object()
                    .ok_or_else(|| LlmMapError::Validation("Invalid tool_call format".into()))?;

                let tool_index = tool_call
                    .get("index")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| LlmMapError::Validation("Missing tool_call index".into()))?;

                let fn_name = tool_call
                    .get("function")
                    .and_then(|v| v.as_object())
                    .and_then(|v| v.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                let fn_arguments = tool_call
                    .get("function")
                    .and_then(|v| v.as_object())
                    .and_then(|v| v.get("arguments"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                let call_id = tool_call.get("id").and_then(|v| v.as_str()).unwrap_or("");

                let (tc_state, has_create) = self.state_mut().get_or_create_tool_call_state(
                    tool_index,
                    call_id.to_string(),
                    fn_name.to_string(),
                );

                // TODO: 如果不是第一个tool_call且是新创建的tool_call， 要发送上一个tool_call结束的生命周期事件
                if self.state().tool_call_count() != 1
                    && has_create
                    && let Some(tc_state) = self.state_mut().get_mut_active_tool_call_state()
                {
                    let seq = self.state_mut().next_sequence();
                    events.push(build_function_call_arguments_done_event(
                        seq,
                        tc_state.output_index,
                        &tc_state.id,
                        &tc_state.args,
                    ));

                    // 标记上一个tool_call结束
                    tc_state.done = true;

                    let seq = self.state_mut().next_sequence();
                    let mut done_item = json!({
                        "id": tc_state.id,
                        "type": "function_call",
                        "status": "completed",
                        "call_id": tc_state.id,
                        "name": tc_state.name,
                        "arguments": tc_state.args,
                    });
                    if let Some(ref ns) = tc_state.namespace {
                        done_item["namespace"] = Value::String(ns.clone());
                    }
                    events.push(build_output_item_done_event(
                        seq,
                        tc_state.output_index,
                        done_item,
                    ));

                    // 将激活的tool_call_index + 1
                    self.state_mut().next_active_tool_call_index();
                }

                if !tc_state.started {
                    let output_index = tc_state.output_index;
                    let item_id = tc_state.id.clone();
                    let name = tc_state.name.clone();

                    tc_state.started = true;

                    let seq = self.state_mut().next_sequence();
                    let mut added_item = json!({
                        "id": &item_id,
                        "type": "function_call",
                        "status": "in_progress",
                        "call_id": &item_id,
                        "name": name,
                        "arguments": ""
                    });
                    if let Some(ref ns) = tc_state.namespace {
                        added_item["namespace"] = Value::String(ns.clone());
                    }
                    events.push(build_output_item_added_event(seq, output_index, added_item));
                }

                let tc = self.state().get_tool_call_state(tool_index);

                let Some(tc) = tc else {
                    continue;
                };

                let output_index = tc.output_index;
                let item_id = tc.id.clone();

                if !fn_arguments.is_empty() {
                    self.state_mut()
                        .push_function_call_args_for_index(tool_index, fn_arguments);

                    let seq = self.state_mut().next_sequence();
                    events.push(build_delta_event(
                        "response.function_call_arguments.delta",
                        seq,
                        output_index,
                        &item_id,
                        fn_arguments,
                        vec![("content_index", Value::Number(0u64.into()))],
                    ));
                }
            }
        }

        if let Some(finish_reason) = finish_reason {
            events.extend(
                self.generate_finishing_events(Some(finish_reason), usage)?
                    .events,
            );
        }

        Ok(events)
    }

    fn genrate_text_done_events(&self) -> Vec<(Value, Option<String>)> {
        let mut events = Vec::with_capacity(4);
        let item_id = self.state().item_id().unwrap();
        let output_index = self.state().item_counter();
        let full_text = self.state().text_buffer().to_string();

        let seq = self.state_mut().next_sequence();
        events.push((
            json!({
                "type": "response.output_text.done",
                "sequence_number": seq,
                "output_index": output_index,
                "item_id": item_id,
                "content_index": 0,
                "text": full_text
            }),
            Some("response.output_text.done".to_string()),
        ));

        let seq = self.state_mut().next_sequence();
        events.push(build_content_part_done_event(
            seq,
            output_index,
            item_id,
            0,
            json!({ "type": "output_text", "text": full_text, "annotations": [], "logprobs": null }),
        ));

        let seq = self.state_mut().next_sequence();
        events.push(build_output_item_done_event(
            seq,
            output_index,
            json!({
                "id": item_id,
                "type": "message",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": full_text, "annotations": [], "logprobs": null }],
                "status": "completed"
            }),
        ));

        self.state_mut().set_text_done();

        events
    }

    fn generate_reasoning_done_events(&self) -> Vec<(Value, Option<String>)> {
        let mut events = Vec::with_capacity(4);
        let item_id = self.state().reasoning_item_id().unwrap();
        let output_index = self.state().item_counter();
        let full_reasoning = self.state().reasoning_buffer().to_string();

        let seq = self.state_mut().next_sequence();
        events.push((
            json!({
                "type": "response.reasoning_summary_text.done",
                "sequence_number": seq,
                "output_index": output_index,
                "item_id": item_id,
                "summary_index": 0,
                "text": full_reasoning
            }),
            Some("response.reasoning_summary_text.done".to_string()),
        ));

        let seq = self.state_mut().next_sequence();
        events.push((
            json!({
                "type": "response.reasoning_summary_part.done",
                "sequence_number": seq,
                "output_index": output_index,
                "item_id": item_id,
                "part": { "type": "summary_text", "text": full_reasoning },
                "summary_index": 0
            }),
            Some("response.reasoning_summary_part.done".to_string()),
        ));

        let seq = self.state_mut().next_sequence();
        events.push(build_output_item_done_event(
            seq,
            output_index,
            json!({
                "id": item_id,
                "type": "reasoning",
                "content": [],
                "summary": [{ "type": "summary_text", "text": full_reasoning }],
                "status": "completed"
            }),
        ));

        self.state_mut().set_reasoning_done();

        events
    }

    fn generate_finishing_events(
        &self,
        finish_reason: Option<&str>,
        usage: Option<&serde_json::Map<String, Value>>,
    ) -> Result<StreamChunkTransform, LlmMapError> {
        let mut events = Vec::with_capacity(16);

        // Close reasoning if started but not done
        if self.state().has_reasoning_started() && !self.state().has_reasoning_done() {
            events.extend(self.generate_reasoning_done_events())
        }

        // Close text if started but not done
        if self.state().has_text_started() && !self.state().has_text_done() {
            events.extend(self.genrate_text_done_events())
        }

        // Close all tool_calls — at most the last one needs closing
        let last_active = self
            .state()
            .tool_call_states()
            .filter(|tc| tc.started && !tc.done)
            .last();
        if let Some(tc) = last_active {
            let output_index = tc.output_index;
            let item_id = &tc.id;
            let func_name = &tc.name;
            let full_args = &tc.args;

            let seq = self.state_mut().next_sequence();
            events.push(build_function_call_arguments_done_event(
                seq,
                output_index,
                item_id,
                full_args,
            ));

            let seq = self.state_mut().next_sequence();
            let mut done_item = json!({
                "id": item_id,
                "type": "function_call",
                "status": "completed",
                "call_id": item_id,
                "name": func_name,
                "arguments": full_args
            });
            if let Some(ref ns) = tc.namespace {
                done_item["namespace"] = Value::String(ns.clone());
            }
            events.push(build_output_item_done_event(seq, output_index, done_item));

            self.state_mut().mark_tool_call_done(tc.index);
        }

        // Build response.completed event
        let status = match finish_reason {
            Some("length") => "incomplete",
            _ => "completed",
        };

        let response_id = self.state().response_id().unwrap_or("");
        let model = self.state().model().unwrap_or("");
        let created_at = self.state().created_at().unwrap_or(0);

        let input_tokens = self.state().input_tokens();
        let output_tokens = self.state().output_tokens();
        let reasoning_tokens = self.state().reasoning_tokens();

        // Update usage from final chunk if available
        let (in_tokens, out_tokens, reason_tokens) = if let Some(u) = usage {
            let input = u
                .get("prompt_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(input_tokens as u64) as u32;
            let output = u
                .get("completion_tokens")
                .or_else(|| u.get("output_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(output_tokens as u64) as u32;
            let reasoning = u
                .get("completion_tokens_details")
                .and_then(|d| d.get("reasoning_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(reasoning_tokens as u64) as u32;
            (input, output, reasoning)
        } else {
            (input_tokens, output_tokens, reasoning_tokens)
        };

        let total_tokens = in_tokens + out_tokens;

        // Build output array with all completed items
        let mut output_items: Vec<Value> = Vec::new();

        // Reasoning item (if exists)
        if self.state().has_reasoning_started() {
            let reasoning_item_id = self
                .state()
                .reasoning_item_id()
                .unwrap_or("resp_reasoning_01");
            let full_reasoning = self.state().reasoning_buffer().to_string();
            output_items.push(json!({
                "id": reasoning_item_id,
                "type": "reasoning",
                "status": "in_progress",
                "summary": [
                    { "type": "summary_text", "text": full_reasoning }
                ]
            }))
        }

        // Message item (if exists)
        if self.state().has_text_started() {
            let msg_item_id = self.state().item_id().unwrap_or("resp_msg_01");
            let full_text = self.state().text_buffer().to_string();
            output_items.push(json!({
                "id": msg_item_id,
                "type": "message",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": full_text, "annotations": [], "logprobs": null }],
                "status": status
            }));
        }

        // Function call items (all tool calls)
        for tc in self.state().tool_call_states() {
            if tc.started {
                let mut fc_item = json!({
                    "id": tc.id,
                    "type": "function_call",
                    "status": "completed",
                    "call_id": tc.id,
                    "name": tc.name,
                    "arguments": tc.args
                });
                if let Some(ref ns) = tc.namespace {
                    fc_item["namespace"] = Value::String(ns.clone());
                }
                output_items.push(fc_item);
            }
        }

        let mut response_obj = build_response_obj(
            response_id,
            model,
            created_at,
            Value::Array(output_items),
            status,
        );
        response_obj["usage"] = json!({
            "input_tokens": in_tokens,
            "input_tokens_details": { "cached_tokens": 0 },
            "output_tokens": out_tokens,
            "output_tokens_details": { "reasoning_tokens": reason_tokens },
            "total_tokens": total_tokens
        });

        events.push((
            json!({
                "type": "response.completed",
                "sequence_number": self.state_mut().next_sequence(),
                "response": response_obj,
                "output_index": 0
            }),
            Some("response.completed".to_string()),
        ));

        // Reset state
        self.state_mut().reset();

        Ok(StreamChunkTransform::new_multi(events))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::converter::stream_test_utils::{
        NormalizedSseData, load_sse_fixture, normalize_stream_events,
    };

    #[test]
    fn test_full_openai_chat_sse_fixture_to_responses_stream() {
        let processor = ChatToResponsesStreamProcessor::new();
        let input_events = load_sse_fixture("input/openai_chat_full.sse").unwrap();
        let expected_events =
            load_sse_fixture("expected/openai_chat_to_responses_full.sse").unwrap();
        let mut output_events = Vec::new();

        for input_event in input_events {
            let NormalizedSseData::Json(chunk) = input_event.data else {
                continue;
            };
            output_events.extend(
                processor
                    .chat_stream_to_responses_stream(chunk)
                    .unwrap()
                    .into_events(),
            );
        }

        assert_eq!(normalize_stream_events(output_events), expected_events);
    }
}
