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

pub fn cleanup_old_usage_files(keep_days: u32) {
    let dir = get_usage_dir();
    let cutoff = Local::now().date_naive() - chrono::Duration::days(keep_days as i64);

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str())
                && name.ends_with(".jsonl")
            {
                let date_str = name.trim_end_matches(".jsonl");
                if let Ok(date) = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
                    && date < cutoff
                {
                    let _ = std::fs::remove_file(&path);
                    tracing::info!("Cleaned up old usage file: {}", name);
                }
            }
        }
    }
}

/// Format a number with smart units: 100 → "100", 1500 → "1.5k", 1500000 → "1.5m"
pub fn format_tokens(n: u32) -> String {
    if n >= 1_000_000 {
        format!("{:.1}m", n as f64 / 1_000_000.0)
    } else if n >= 1000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_tokens() {
        assert_eq!(format_tokens(100), "100");
        assert_eq!(format_tokens(1500), "1.5k");
        assert_eq!(format_tokens(15000), "15.0k");
        assert_eq!(format_tokens(1500000), "1.5m");
        assert_eq!(format_tokens(15000000), "15.0m");
        assert_eq!(format_tokens(999), "999");
        assert_eq!(format_tokens(1000), "1.0k");
    }
}
