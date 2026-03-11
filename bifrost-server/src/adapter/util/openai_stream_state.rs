//! Shared stream state for OpenAI → Anthropic stream conversion
//!
//! This module provides the state management needed to convert OpenAI-format
//! streaming responses to Anthropic-format streaming events.

use std::collections::HashMap;

/// Content type for tracking which blocks have been sent
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ContentType {
    Thinking,
    Text,
    ToolCall,
}

/// Unified stream state for OpenAI → Anthropic stream conversion
/// All state protected by single Mutex for thread safety
pub struct OpenAIStreamState {
    /// Block index for thinking content (usize::MAX = not started)
    thinking_block_index: usize,
    /// Block index for text content (usize::MAX = not started)
    text_block_index: usize,
    /// Next block index to assign
    next_block_index: usize,
    /// Whether message_start event has been sent
    has_sent_message_start: bool,
    /// Track which content blocks have been started
    blocks_started: Vec<ContentType>,
    /// Map OpenAI tool_call index → Anthropic block index
    tool_call_map: HashMap<usize, usize>,
    /// Current active block index (usize::MAX = no active block)
    /// Used to track when to send content_block_stop before starting new block
    current_active_block_index: usize,
}

impl OpenAIStreamState {
    /// Create a new stream state
    pub fn new() -> Self {
        Self {
            thinking_block_index: usize::MAX,
            text_block_index: usize::MAX,
            next_block_index: 0,
            has_sent_message_start: false,
            blocks_started: Vec::new(),
            tool_call_map: HashMap::new(),
            current_active_block_index: usize::MAX,
        }
    }

    /// Reset state for next request
    pub fn reset(&mut self) {
        self.thinking_block_index = usize::MAX;
        self.text_block_index = usize::MAX;
        self.next_block_index = 0;
        self.has_sent_message_start = false;
        self.blocks_started.clear();
        self.tool_call_map.clear();
        self.current_active_block_index = usize::MAX;
    }

    /// Get the thinking block index
    pub fn thinking_block_index(&self) -> usize {
        self.thinking_block_index
    }

    /// Get the text block index
    pub fn text_block_index(&self) -> usize {
        self.text_block_index
    }

    /// Get the next block index to assign
    pub fn next_block_index(&self) -> usize {
        self.next_block_index
    }

    /// Check if message_start has been sent
    pub fn has_sent_message_start(&self) -> bool {
        self.has_sent_message_start
    }

    /// Get the list of started blocks
    pub fn blocks_started(&self) -> &[ContentType] {
        &self.blocks_started
    }

    /// Get all tool_call block indices
    pub fn tool_call_indices(&self) -> Vec<usize> {
        self.tool_call_map.values().copied().collect()
    }

    /// Set thinking block index and mark as started
    pub fn start_thinking_block(&mut self, index: usize) {
        self.thinking_block_index = index;
        self.blocks_started.push(ContentType::Thinking);
    }

    /// Set text block index and mark as started
    pub fn start_text_block(&mut self, index: usize) {
        self.text_block_index = index;
        self.blocks_started.push(ContentType::Text);
    }

    /// Get or create tool_call block index for a given OpenAI tool_call index
    /// Returns (block_index, needs_start) where needs_start indicates if this is a new tool_call
    pub fn get_or_create_tool_call_block(&mut self, tool_call_index: usize) -> (usize, bool) {
        if let Some(&block_index) = self.tool_call_map.get(&tool_call_index) {
            // Already started, just return the block index
            (block_index, false)
        } else {
            // New tool_call, allocate a new block index
            let block_index = self.next_block_index;
            self.next_block_index += 1;
            self.tool_call_map.insert(tool_call_index, block_index);
            self.blocks_started.push(ContentType::ToolCall);
            (block_index, true)
        }
    }

    /// Increment next block index
    pub fn increment_next_block_index(&mut self) -> usize {
        let index = self.next_block_index;
        self.next_block_index += 1;
        index
    }
    /// Set message_start as sent
    pub fn set_message_start_sent(&mut self) {
        self.has_sent_message_start = true;
    }

    /// Get current active block index
    pub fn current_active_block_index(&self) -> usize {
        self.current_active_block_index
    }

    /// Set current active block index, returns old value
    /// Returns Some(old_index) if there was a previous active block (different from new_index)
    /// Returns None if no previous active block or same as new index
    pub fn set_current_active_block(&mut self, new_index: usize) -> Option<usize> {
        let old = self.current_active_block_index;
        self.current_active_block_index = new_index;
        if old != usize::MAX && old != new_index {
            Some(old)
        } else {
            None
        }
    }
}

impl Default for OpenAIStreamState {
    fn default() -> Self {
        Self::new()
    }
}
