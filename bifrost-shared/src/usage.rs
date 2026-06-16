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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deployment: Option<String>,
    pub model: String,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_tokens: Option<u32>,
}

impl UsageRecord {
    pub fn new(
        provider_id: &str,
        model: &str,
        prompt_tokens: u32,
        completion_tokens: u32,
        cached_tokens: Option<u32>,
    ) -> Self {
        Self {
            time: Local::now().format("%H:%M:%S").to_string(),
            provider: provider_id.to_string(),
            deployment: None,
            model: model.to_string(),
            prompt_tokens,
            completion_tokens,
            total_tokens: prompt_tokens + completion_tokens,
            cached_tokens,
        }
    }

    pub fn with_deployment(
        provider_id: &str,
        deployment_id: &str,
        model: &str,
        prompt_tokens: u32,
        completion_tokens: u32,
        cached_tokens: Option<u32>,
    ) -> Self {
        let mut record = Self::new(
            provider_id,
            model,
            prompt_tokens,
            completion_tokens,
            cached_tokens,
        );
        record.deployment = Some(deployment_id.to_string());
        record
    }

    pub fn write(&self) -> std::io::Result<()> {
        let _guard = WRITE_LOCK.lock().unwrap();
        let dir = get_usage_dir();
        let date = Local::now().format("%Y-%m-%d").to_string();
        let filename = format!("{}.jsonl", date);
        let path = dir.join(filename);

        fs::create_dir_all(dir)?;
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        // Write the JSON line and newline as a single write_all call so that a crash
        // between partial writes cannot produce a truncated line in the file.
        let line = format!("{}\n", serde_json::to_string(self)?);
        file.write_all(line.as_bytes())
    }
}

pub fn extract_openai_usage(response: &serde_json::Value) -> Option<(u32, u32, Option<u32>)> {
    let usage = response.get("usage")?.as_object()?;
    Some(extract_openai_usage_fields(usage))
}

fn extract_openai_usage_fields(
    usage: &serde_json::Map<String, serde_json::Value>,
) -> (u32, u32, Option<u32>) {
    let prompt = if usage.contains_key("prompt_tokens") {
        usage
            .get("prompt_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
    } else {
        usage
            .get("input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
    } as u32;
    let completion = if usage.contains_key("completion_tokens") {
        usage
            .get("completion_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
    } else {
        usage
            .get("output_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
    } as u32;
    let cached = usage
        .get("prompt_tokens_details")
        .and_then(|d| d.get("cached_tokens"))
        .or_else(|| usage.get("prompt_cache_hit_tokens"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .filter(|&v| v > 0);
    (prompt, completion, cached)
}

pub fn extract_anthropic_usage(response: &serde_json::Value) -> Option<(u32, u32, Option<u32>)> {
    let usage = response.get("usage")?.as_object()?;
    let input = usage.get("input_tokens")?.as_u64().unwrap_or(0) as u32;
    let output = usage.get("output_tokens")?.as_u64().unwrap_or(0) as u32;
    let cache_read = usage
        .get("cache_read_input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let cache_creation = usage
        .get("cache_creation_input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let prompt = input + cache_read + cache_creation;
    let cached = (cache_read > 0).then_some(cache_read);
    Some((prompt, output, cached))
}

pub fn record_usage(
    response: &serde_json::Value,
    provider_id: &str,
    endpoint: Endpoint,
    model: &str,
    deployment_id: Option<&str>,
) {
    let (prompt_tokens, completion_tokens, cached_tokens) = match endpoint {
        Endpoint::OpenAI => extract_openai_usage(response).unwrap_or((0, 0, None)),
        Endpoint::Anthropic => extract_anthropic_usage(response).unwrap_or((0, 0, None)),
    };

    let record = match deployment_id {
        Some(deployment_id) => UsageRecord::with_deployment(
            provider_id,
            deployment_id,
            model,
            prompt_tokens,
            completion_tokens,
            cached_tokens,
        ),
        None => UsageRecord::new(
            provider_id,
            model,
            prompt_tokens,
            completion_tokens,
            cached_tokens,
        ),
    };
    if let Err(e) = record.write() {
        tracing::warn!("Failed to write usage record: {}", e);
    }
}

pub fn record_stream_usage(
    provider_id: &str,
    model: &str,
    deployment_id: Option<&str>,
    prompt_tokens: u32,
    completion_tokens: u32,
    cached_tokens: Option<u32>,
) {
    let record = match deployment_id {
        Some(deployment_id) => UsageRecord::with_deployment(
            provider_id,
            deployment_id,
            model,
            prompt_tokens,
            completion_tokens,
            cached_tokens,
        ),
        None => UsageRecord::new(
            provider_id,
            model,
            prompt_tokens,
            completion_tokens,
            cached_tokens,
        ),
    };
    if let Err(e) = record.write() {
        tracing::warn!("Failed to write usage record: {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::{extract_anthropic_usage, extract_openai_usage};
    use serde_json::json;

    #[test]
    fn openai_usage_extracts_cached_prompt_tokens() {
        let response = json!({
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 25,
                "prompt_tokens_details": {
                    "cached_tokens": 40
                }
            }
        });

        assert_eq!(extract_openai_usage(&response), Some((100, 25, Some(40))));
    }

    #[test]
    fn openai_usage_extracts_top_level_prompt_cache_hit_tokens() {
        let response = json!({
            "usage": {
                "prompt_tokens": 5199,
                "completion_tokens": 41,
                "total_tokens": 5240,
                "completion_tokens_details": {
                    "reasoning_tokens": 19
                },
                "prompt_cache_hit_tokens": 5120,
                "prompt_cache_miss_tokens": 79
            }
        });

        assert_eq!(
            extract_openai_usage(&response),
            Some((5199, 41, Some(5120)))
        );
    }

    #[test]
    fn openai_usage_prefers_standard_tokens_over_zero_aliases() {
        let response = json!({
            "usage": {
                "prompt_tokens": 31422,
                "completion_tokens": 107,
                "total_tokens": 31522,
                "prompt_tokens_details": {
                    "cached_tokens": 0,
                    "text_tokens": 0,
                    "audio_tokens": 0,
                    "image_tokens": 0
                },
                "completion_tokens_details": {
                    "text_tokens": 0,
                    "audio_tokens": 0,
                    "reasoning_tokens": 0
                },
                "input_tokens": 0,
                "output_tokens": 0,
                "input_tokens_details": null,
                "claude_cache_creation_5_m_tokens": 0,
                "claude_cache_creation_1_h_tokens": 0
            }
        });

        assert_eq!(extract_openai_usage(&response), Some((31422, 107, None)));
    }

    #[test]
    fn openai_usage_falls_back_to_response_token_aliases() {
        let response = json!({
            "usage": {
                "input_tokens": 100,
                "output_tokens": 25,
                "total_tokens": 125
            }
        });

        assert_eq!(extract_openai_usage(&response), Some((100, 25, None)));
    }

    #[test]
    fn anthropic_usage_counts_all_input_token_kinds() {
        let response = json!({
            "usage": {
                "input_tokens": 60,
                "cache_read_input_tokens": 30,
                "cache_creation_input_tokens": 10,
                "output_tokens": 25
            }
        });

        assert_eq!(
            extract_anthropic_usage(&response),
            Some((100, 25, Some(30)))
        );
    }

    #[test]
    fn anthropic_usage_omits_zero_cached_tokens() {
        let response = json!({
            "usage": {
                "input_tokens": 60,
                "cache_read_input_tokens": 0,
                "cache_creation_input_tokens": 10,
                "output_tokens": 25
            }
        });

        assert_eq!(extract_anthropic_usage(&response), Some((70, 25, None)));
    }
}
