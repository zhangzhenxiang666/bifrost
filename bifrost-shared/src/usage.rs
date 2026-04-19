use chrono::Local;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::OnceLock;

use crate::Endpoint;

static USAGE_DIR: OnceLock<PathBuf> = OnceLock::new();
static WRITE_LOCK: Mutex<()> = Mutex::new(());

fn get_usage_dir() -> &'static PathBuf {
    USAGE_DIR.get_or_init(|| {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".bifrost")
            .join("usage")
    })
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct UsageRecord {
    pub time: String,
    pub provider: String,
    pub model: String,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

impl UsageRecord {
    pub fn new(provider_id: &str, model: &str, prompt_tokens: u32, completion_tokens: u32) -> Self {
        Self {
            time: Local::now().format("%H:%M:%S").to_string(),
            provider: provider_id.to_string(),
            model: model.to_string(),
            prompt_tokens,
            completion_tokens,
            total_tokens: prompt_tokens + completion_tokens,
        }
    }

    pub fn write(&self) -> std::io::Result<()> {
        let _guard = WRITE_LOCK.lock().unwrap();
        let dir = get_usage_dir();
        let date = Local::now().format("%Y-%m-%d").to_string();
        let filename = format!("{}.jsonl", date);
        let path = dir.join(filename);

        fs::create_dir_all(dir)?;
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        writeln!(file, "{}", serde_json::to_string(self)?)
    }
}

pub fn extract_openai_usage(response: &serde_json::Value) -> Option<(u32, u32)> {
    let usage = response.get("usage")?.as_object()?;
    let prompt = usage.get("prompt_tokens")?.as_u64().unwrap_or(0) as u32;
    let completion = usage.get("completion_tokens")?.as_u64().unwrap_or(0) as u32;
    Some((prompt, completion))
}

pub fn extract_anthropic_usage(response: &serde_json::Value) -> Option<(u32, u32)> {
    let usage = response.get("usage")?.as_object()?;
    let input = usage.get("input_tokens")?.as_u64().unwrap_or(0) as u32;
    let output = usage.get("output_tokens")?.as_u64().unwrap_or(0) as u32;
    Some((input, output))
}

pub fn record_usage(
    response: &serde_json::Value,
    provider_id: &str,
    endpoint: Endpoint,
    model: &str,
) {
    let (prompt_tokens, completion_tokens) = match endpoint {
        Endpoint::OpenAI => extract_openai_usage(response).unwrap_or((0, 0)),
        Endpoint::Anthropic => extract_anthropic_usage(response).unwrap_or((0, 0)),
    };

    let record = UsageRecord::new(provider_id, model, prompt_tokens, completion_tokens);
    if let Err(e) = record.write() {
        tracing::warn!("Failed to write usage record: {}", e);
    }
}

pub fn record_stream_usage(
    provider_id: &str,
    model: &str,
    prompt_tokens: u32,
    completion_tokens: u32,
) {
    let record = UsageRecord::new(provider_id, model, prompt_tokens, completion_tokens);
    if let Err(e) = record.write() {
        tracing::warn!("Failed to write usage record: {}", e);
    }
}
