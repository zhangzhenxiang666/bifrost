//! Shared stream state for OpenAI → Anthropic stream conversion
//!
//! This module provides the state management needed to convert OpenAI-format
//! streaming responses to Anthropic-format streaming events.

/// Content type for tracking which blocks have been sent
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ContentType {
    Thinking,
    Text,
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
        }
    }

    /// Reset state for next request
    pub fn reset(&mut self) {
        self.thinking_block_index = usize::MAX;
        self.text_block_index = usize::MAX;
        self.next_block_index = 0;
        self.has_sent_message_start = false;
        self.blocks_started.clear();
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
}

impl Default for OpenAIStreamState {
    fn default() -> Self {
        Self::new()
    }
}
