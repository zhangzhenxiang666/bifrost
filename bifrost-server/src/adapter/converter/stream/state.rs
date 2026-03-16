//! Shared stream state for OpenAI → Anthropic stream conversion
//!
//! This module provides the state management needed to convert OpenAI-format
//! streaming responses to Anthropic-format streaming events.

/// Unified stream state for OpenAI → Anthropic stream conversion
pub struct OpenAIStreamState {
    /// Next block index to assign
    next_block_index: usize,
    /// OpenAI tool_call index → Anthropic block index.
    /// Index into Vec = OpenAI tool_call index; value = Anthropic block index.
    /// usize::MAX means the slot is unallocated.
    /// OpenAI tool_call indices are always 0-based consecutive integers,
    /// so a Vec is strictly faster than a HashMap here.
    tool_call_blocks: Vec<usize>,
    /// Current active block index (usize::MAX = no active block)
    current_active_block_index: usize,
    /// Flags to mark the start of message, thinking, and text blocks
    flags: u8,
}

const MESSAGE_STARTED: u8 = 0b001;
const THINKING_STARTED: u8 = 0b010;
const TEXT_STARTED: u8 = 0b100;

impl OpenAIStreamState {
    /// Create a new stream state
    pub fn new() -> Self {
        Self {
            next_block_index: 0,
            tool_call_blocks: Vec::new(),
            current_active_block_index: usize::MAX,
            flags: 0,
        }
    }

    /// Reset state for next request.
    /// `clear()` retains the Vec's heap allocation so it can be reused across resets.
    pub fn reset(&mut self) {
        self.next_block_index = 0;
        self.flags = 0;
        self.tool_call_blocks.clear();
        self.current_active_block_index = usize::MAX;
    }

    // ── Getters ───────────────────────────────────────────────────────────────

    pub fn current_active_block_index(&self) -> usize {
        self.current_active_block_index
    }

    // ── Compound mutators (one UnsafeCell round-trip each) ────────────────────

    pub fn has_message_started(&self) -> bool {
        self.flags & MESSAGE_STARTED != 0
    }

    pub fn has_thinking_started(&self) -> bool {
        self.flags & THINKING_STARTED != 0
    }

    pub fn has_text_started(&self) -> bool {
        self.flags & TEXT_STARTED != 0
    }

    pub fn set_message_started(&mut self) {
        self.flags |= MESSAGE_STARTED;
    }

    /// Allocate and initialise a new thinking block in one operation.
    ///
    /// Thinking is always the very first block, so there is never a previous
    /// active block to close; `current_active_block_index` is set directly.
    ///
    /// Returns the new block index.
    pub fn init_thinking_block(&mut self) -> usize {
        let index = self.next_block_index;
        self.next_block_index += 1;
        self.flags |= THINKING_STARTED;
        self.current_active_block_index = index;
        index
    }

    /// Allocate and initialise a new text block in one operation.
    ///
    /// Returns `(new_index, old_active)`.  
    /// If `old_active` is `Some(idx)` the caller must emit a `content_block_stop`
    /// for `idx` before emitting the new `content_block_start`.
    pub fn init_text_block(&mut self) -> (usize, Option<usize>) {
        let index = self.next_block_index;
        self.next_block_index += 1;
        self.flags |= TEXT_STARTED;
        let old = self.current_active_block_index;
        self.current_active_block_index = index;
        let old_opt = if old != usize::MAX && old != index {
            Some(old)
        } else {
            None
        };
        (index, old_opt)
    }

    /// Get or create a tool-call block for the given OpenAI `tool_call.index`.
    ///
    /// Returns `(block_index, needs_start)`:
    /// - `needs_start = true`  → first time we see this tool_call; caller must emit
    ///   `content_block_start` (and close the previous active block if any).
    /// - `needs_start = false` → continuation; just emit the delta.
    pub fn get_or_create_tool_call_block(&mut self, tool_call_index: usize) -> (usize, bool) {
        // Fast path: already allocated.
        if let Some(&block) = self.tool_call_blocks.get(tool_call_index)
            && block != usize::MAX
        {
            return (block, false);
        }

        // Slow path: first time we see this index.
        let block_index = self.next_block_index;
        self.next_block_index += 1;
        if tool_call_index >= self.tool_call_blocks.len() {
            self.tool_call_blocks
                .resize(tool_call_index + 1, usize::MAX);
        }
        self.tool_call_blocks[tool_call_index] = block_index;
        (block_index, true)
    }

    /// Switch the current active block to `new_index`.
    ///
    /// Returns `Some(old_index)` when there was a different active block that
    /// the caller must close with a `content_block_stop`.
    /// Returns `None` when there was no active block or it is the same index.
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
