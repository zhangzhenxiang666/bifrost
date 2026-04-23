use std::fs;

use crate::config::get_config_path;

pub fn validate_config() -> Result<(), String> {
    let config_path = get_config_path().map_err(|e| e.to_string())?;

    if !config_path.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(&config_path).map_err(|e| e.to_string())?;
    let config: bifrost_shared::Config =
        toml::from_str(&content).map_err(|e| format!("TOML syntax error: {}", e))?;
    config.validate().map_err(|e| e.to_string())?;

    Ok(())
}
