use std::fs;

use crate::config::get_config_path;

const PREDEFINED_ADAPTERS: &[&str] = &[
    "passthrough",
    "anthropic_to_openai",
    "anthropic-to-openai",
    "responses_to_chat",
    "responses-to-chat",
];

pub fn validate_config() -> Result<(), String> {
    let config_path = get_config_path().map_err(|e| e.to_string())?;

    if !config_path.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(&config_path).map_err(|e| e.to_string())?;

    let config: bifrost_config::Config =
        toml::from_str(&content).map_err(|e| format!("TOML syntax error: {}", e))?;

    // Semantic checks - add more validators here as needed
    check_adapter_names(&config)?;
    // TODO: Add more semantic validators below:
    // check_model_names(&config)?;
    // check_base_urls(&config)?;
    // check_duplicate_providers(&config)?;

    Ok(())
}

fn check_adapter_names(config: &bifrost_config::Config) -> Result<(), String> {
    for adapter in config.used_adapters() {
        if !PREDEFINED_ADAPTERS.contains(&adapter.as_str()) {
            return Err(format!(
                "Unknown adapter '{}'. Valid adapters are: {:?}",
                adapter, PREDEFINED_ADAPTERS
            ));
        }
    }
    Ok(())
}
