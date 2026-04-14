//! Shared stream state for Chat → Responses SSE conversion
//!
//! This module provides the state machine for tracking Chat-to-Responses
//! streaming event conversion state.

/// Per-tool-call state for tracking individual tool call lifecycle in SSE output.
///
/// Each tool call in a `tool_calls` array gets its own `ToolCallState` to independently
/// track whether it has emitted its `output_item.added` and `output_item.done` events.
#[derive(Debug, Clone)]
pub struct ToolCallState {
    /// Index in the Chat API's `tool_calls` array
    pub index: u64,
    /// The call_id (e.g., "call_d500d156ea0245ec9a3cfcc7")
    pub id: String,
    /// Function name
    pub name: String,
    /// Accumulated arguments (JSON string)
    pub args: String,
    /// Whether `output_item.added` has been emitted for this tool
    pub started: bool,
    /// Whether `output_item.done` has been emitted for this tool
    pub done: bool,
    /// The responses API output_index for this tool call
    pub output_index: u64,
}

impl ToolCallState {
    fn new(index: u64, id: String, name: String, output_index: u64) -> Self {
        Self {
            index,
            id,
            name,
            args: String::new(),
            started: false,
            done: false,
            output_index,
        }
    }
}

/// Unified stream state for Chat → Responses SSE conversion
pub struct ChatToResponsesStreamState {
    sequence_number: u64,
    response_id: Option<String>,
    model: Option<String>,
    created_at: Option<u64>,
    flags: u8,
    item_id: Option<String>,
    reasoning_item_id: Option<String>,
    text_buffer: String,
    reasoning_buffer: String,
    input_tokens: u32,
    output_tokens: u32,
    reasoning_tokens: u32,
    tool_call_states: std::collections::HashMap<u64, ToolCallState>,
    active_tool_call_index: u64,
    /// Counter for generating sequential output_index values for new output items.
    /// Starts at 0 for reasoning, increments for each new item.
    item_counter: u64,
}

const CREATED_SENT: u8 = 0b0000_0001;
const IN_PROGRESS_SENT: u8 = 0b0000_0010;
#[expect(dead_code)]
const MESSAGE_ITEM_STARTED: u8 = 0b0000_0100;
const TEXT_STARTED: u8 = 0b0000_1000;
const TEXT_DONE: u8 = 0b0001_0000;
const REASONING_STARTED: u8 = 0b0010_0000;
const REASONING_DONE: u8 = 0b0100_0000;
// FUNCTION_CALL_STARTED and FUNCTION_CALL_DONE removed — now tracked per ToolCallState

impl ChatToResponsesStreamState {
    pub fn new() -> Self {
        Self {
            sequence_number: 0,
            response_id: None,
            model: None,
            created_at: None,
            flags: 0,
            item_id: None,
            reasoning_item_id: None,
            text_buffer: String::new(),
            reasoning_buffer: String::new(),
            input_tokens: 0,
            output_tokens: 0,
            reasoning_tokens: 0,
            item_counter: 0,
            tool_call_states: std::collections::HashMap::new(),
            active_tool_call_index: 0,
        }
    }

    pub fn reset(&mut self) {
        self.sequence_number = 0;
        self.response_id = None;
        self.model = None;
        self.created_at = None;
        self.flags = 0;
        self.item_id = None;
        self.reasoning_item_id = None;
        self.text_buffer.clear();
        self.reasoning_buffer.clear();
        self.input_tokens = 0;
        self.output_tokens = 0;
        self.reasoning_tokens = 0;
        self.tool_call_states = std::collections::HashMap::new();
        self.active_tool_call_index = 0;
        self.item_counter = 0;
    }

    pub fn sequence_number(&self) -> u64 {
        self.sequence_number
    }

    pub fn next_sequence(&mut self) -> u64 {
        let current = self.sequence_number;
        self.sequence_number += 1;
        current
    }

    pub fn response_id(&self) -> Option<&str> {
        self.response_id.as_deref()
    }

    pub fn set_first_chunk_meta(&mut self, id: String, model: String) {
        if self.response_id.is_none() {
            self.response_id = Some(id);
        }
        if self.model.is_none() {
            self.model = Some(model);
        }
        if self.created_at.is_none() {
            self.created_at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|d| d.as_secs());
        }
    }

    pub fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    pub fn created_at(&self) -> Option<u64> {
        self.created_at
    }

    pub fn item_id(&self) -> Option<&str> {
        self.item_id.as_deref()
    }

    pub fn reasoning_item_id(&self) -> Option<&str> {
        self.reasoning_item_id.as_deref()
    }

    // ── text buffer ──

    pub fn text_buffer(&self) -> &str {
        &self.text_buffer
    }

    pub fn push_text(&mut self, text: &str) {
        self.text_buffer.push_str(text);
    }

    // ── reasoning buffer ──

    pub fn push_reasoning(&mut self, text: &str) {
        self.reasoning_buffer.push_str(text);
    }

    pub fn reasoning_buffer(&self) -> &str {
        &self.reasoning_buffer
    }

    // ── function call compatibility layer (delegates to first ToolCallState) ──

    /// Appends args to the tool call at the given index.
    pub fn push_function_call_args_for_index(&mut self, index: u64, args: &str) {
        if let Some(tc) = self.tool_call_states.get_mut(&index) {
            tc.args.push_str(args);
        }
    }

    // ── tool call state management ──

    /// Returns an iterator over all tool call states.
    pub fn tool_call_states(&self) -> impl Iterator<Item = &ToolCallState> {
        self.tool_call_states.values()
    }

    pub fn tool_call_count(&self) -> usize {
        self.tool_call_states.len()
    }

    pub fn get_or_create_tool_call_state(
        &mut self,
        index: u64,
        id: String,
        name: String,
    ) -> (&mut ToolCallState, bool) {
        use std::collections::hash_map::Entry;

        // 提前计算 output_index 和存在性，避免 entry() 可变借用与 flag 读取的借用冲突
        let is_new = !self.tool_call_states.contains_key(&index);
        let output_index = if is_new {
            if !self.has_reasoning_started()
                && !self.has_text_started()
                && self.tool_call_states.is_empty()
            {
                self.item_counter
            } else {
                self.item_counter += 1;
                self.item_counter
            }
        } else {
            0
        };

        match self.tool_call_states.entry(index) {
            Entry::Occupied(entry) => (entry.into_mut(), false),
            Entry::Vacant(entry) => {
                let new_tc = ToolCallState::new(index, id, name, output_index);
                (entry.insert(new_tc), true)
            }
        }
    }

    pub fn get_active_tool_call_state(&self) -> Option<&ToolCallState> {
        self.tool_call_states.get(&self.active_tool_call_index)
    }

    pub fn get_mut_active_tool_call_state(&mut self) -> Option<&mut ToolCallState> {
        self.tool_call_states.get_mut(&self.active_tool_call_index)
    }

    pub fn next_active_tool_call_index(&mut self) -> u64 {
        let current = self.active_tool_call_index;
        self.active_tool_call_index += 1;
        current
    }

    pub fn get_tool_call_state(&self, index: u64) -> Option<&ToolCallState> {
        self.tool_call_states.get(&index)
    }

    /// Increments and returns the next output_index for a new output item.
    /// The first call returns 0 (for reasoning), the second returns 1 (message), etc.
    pub fn next_output_index(&mut self) -> u64 {
        let current = self.item_counter;
        self.item_counter += 1;
        current
    }

    /// Returns the current item counter value without incrementing.
    pub fn item_counter(&self) -> u64 {
        self.item_counter
    }

    /// Sets the current item counter value. Useful for restoring state.
    pub fn set_item_counter(&mut self, counter: u64) {
        self.item_counter = counter;
    }

    /// Marks the tool call at the given index as done.
    pub fn mark_tool_call_done(&mut self, index: u64) {
        if let Some(tc) = self.tool_call_states.get_mut(&index) {
            tc.done = true;
        }
    }

    // ── token usage ──

    pub fn input_tokens(&self) -> u32 {
        self.input_tokens
    }

    pub fn output_tokens(&self) -> u32 {
        self.output_tokens
    }

    pub fn reasoning_tokens(&self) -> u32 {
        self.reasoning_tokens
    }

    pub fn set_usage(&mut self, input_tokens: u32, output_tokens: u32, reasoning_tokens: u32) {
        self.input_tokens = input_tokens;
        self.output_tokens = output_tokens;
        self.reasoning_tokens = reasoning_tokens;
    }

    // ── bitmask flags ──

    pub fn has_created_sent(&self) -> bool {
        self.flags & CREATED_SENT != 0
    }

    pub fn set_created_sent(&mut self) {
        self.flags |= CREATED_SENT;
    }

    pub fn has_in_progress_sent(&self) -> bool {
        self.flags & IN_PROGRESS_SENT != 0
    }

    pub fn set_in_progress_sent(&mut self) {
        self.flags |= IN_PROGRESS_SENT;
    }

    pub fn has_text_started(&self) -> bool {
        self.flags & TEXT_STARTED != 0
    }

    pub fn set_text_started(&mut self) {
        self.flags |= TEXT_STARTED;
    }

    pub fn has_text_done(&self) -> bool {
        self.flags & TEXT_DONE != 0
    }

    pub fn set_text_done(&mut self) {
        self.flags |= TEXT_DONE;
    }

    pub fn has_reasoning_started(&self) -> bool {
        self.flags & REASONING_STARTED != 0
    }

    pub fn set_reasoning_started(&mut self) {
        self.flags |= REASONING_STARTED;
    }

    pub fn has_reasoning_done(&self) -> bool {
        self.flags & REASONING_DONE != 0
    }

    pub fn set_reasoning_done(&mut self) {
        self.flags |= REASONING_DONE;
    }

    // ── item ID generation ──

    pub fn set_item_id(&mut self, id: String) {
        self.item_id = Some(id);
    }

    pub fn set_reasoning_item_id(&mut self, id: String) {
        self.reasoning_item_id = Some(id);
    }
}

impl Default for ChatToResponsesStreamState {
    fn default() -> Self {
        Self::new()
    }
}
