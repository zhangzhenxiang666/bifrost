pub mod request;
pub mod response;
pub mod stream;

use rand::Rng;

const ITEM_ID_CHARSET: &[u8; 62] =
    b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

fn create_item_id(prefix: &str) -> String {
    let mut rng = rand::thread_rng();
    let mut id = String::with_capacity(prefix.len() + 1 + 16);
    id.push_str(prefix);
    id.push('_');
    for _ in 0..16 {
        id.push(ITEM_ID_CHARSET[rng.gen_range(0..ITEM_ID_CHARSET.len())] as char);
    }
    id
}

/// Stores namespace prefixes extracted from Responses API namespace tools.
///
/// Used to reverse-map Chat API tool call names back to (namespace, short_name).
#[derive(Default, Clone)]
pub struct NamespaceMappings {
    /// Namespace prefixes sorted by length descending (longest first).
    prefixes: Vec<String>,
}

impl NamespaceMappings {
    pub fn new() -> Self {
        Self {
            prefixes: Vec::new(),
        }
    }

    /// Add a namespace prefix for reverse lookup.
    pub fn add_namespace(&mut self, namespace: String) {
        self.prefixes.push(namespace);
        // Sort by length descending so longest prefix matches first.
        self.prefixes.sort_by_key(|b| std::cmp::Reverse(b.len()));
    }

    /// Returns the list of namespace prefixes (for inspection/debugging).
    pub fn prefixes(&self) -> &[String] {
        &self.prefixes
    }

    /// Given a full tool name (e.g. `"mcp__weather__get_forecast"`),
    /// return `(namespace, short_name)` if any prefix matches.
    pub fn split_name(&self, full_name: &str) -> Option<(String, String)> {
        for prefix in &self.prefixes {
            if let Some(rest) = full_name.strip_prefix(prefix.as_str()) {
                return Some((prefix.clone(), rest.to_string()));
            }
        }
        None
    }
}
