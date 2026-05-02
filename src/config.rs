//! Configuration management for bifrost CLI

use anyhow::{Context, Result};
use chrono::{Local, TimeZone};
use std::fs;
use std::path::PathBuf;

/// Default directory for bifrost configuration and logs
pub const BIFROST_DIR: &str = ".bifrost";
pub const CONFIG_FILE: &str = "config.toml";
pub const LOGS_DIR: &str = "logs";
pub const PID_FILE: &str = "bifrost.pid";
pub const LOG_RETENTION_DAYS: i64 = 30;

/// Get the bifrost directory path (~/.bifrost)
pub fn get_bifrost_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Failed to get home directory")?;
    Ok(home.join(BIFROST_DIR))
}

/// Get the config file path (~/.bifrost/config.toml)
pub fn get_config_path() -> Result<PathBuf> {
    Ok(get_bifrost_dir()?.join(CONFIG_FILE))
}

/// Get the logs directory path (~/.bifrost/logs)
pub fn get_logs_dir() -> Result<PathBuf> {
    Ok(get_bifrost_dir()?.join(LOGS_DIR))
}

/// Get the PID file path (~/.bifrost/bifrost.pid)
pub fn get_pid_file_path() -> Result<PathBuf> {
    Ok(get_bifrost_dir()?.join(PID_FILE))
}

/// Get today's log file path
pub fn get_today_log_path() -> Result<PathBuf> {
    let date = Local::now().format("%Y-%m-%d").to_string();
    Ok(get_logs_dir()?.join(format!("{}.log", date)))
}

/// Initialize the bifrost directory structure
pub fn init_bifrost_dir() -> Result<()> {
    let bifrost_dir = get_bifrost_dir()?;
    let logs_dir = get_logs_dir()?;

    // Create ~/.bifrost directory if it doesn't exist
    if !bifrost_dir.exists() {
        fs::create_dir_all(&bifrost_dir)
            .context(format!("Failed to create directory: {:?}", bifrost_dir))?;
    }

    // Create ~/.bifrost/logs directory if it doesn't exist
    if !logs_dir.exists() {
        fs::create_dir_all(&logs_dir)
            .context(format!("Failed to create directory: {:?}", logs_dir))?;
    }

    // Create empty config.toml if it doesn't exist
    let config_path = get_config_path()?;
    if !config_path.exists() {
        fs::write(&config_path, "")
            .context(format!("Failed to create config file: {:?}", config_path))?;
    }

    Ok(())
}

/// Clean up old log files (older than 30 days)
pub fn cleanup_old_logs() -> Result<usize> {
    let logs_dir = get_logs_dir()?;

    if !logs_dir.exists() {
        return Ok(0);
    }

    let mut deleted_count = 0;
    let now = Local::now();

    for entry in
        fs::read_dir(&logs_dir).context(format!("Failed to read logs directory: {:?}", logs_dir))?
    {
        let entry = entry?;
        let path = entry.path();

        // Only process .log files
        if path.extension().and_then(|s| s.to_str()) != Some("log") {
            continue;
        }

        let filename = path.file_name().and_then(|s| s.to_str()).unwrap_or("");

        // New format: YYYY-MM-DD.log or YYYY-MM-DD.log.N (rolling files)
        // Extract date from filename
        let date_str = if filename.ends_with(".log") {
            // Check if it's a rolling file (e.g., 2026-05-02.log.1)
            filename.split('.').next().unwrap_or("")
        } else {
            continue;
        };

        if let Ok(log_date) = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
            let log_date = log_date.and_hms_opt(0, 0, 0).unwrap();
            let log_date = Local.from_local_datetime(&log_date).single().unwrap();
            let age = now.signed_duration_since(log_date);

            if age.num_days() > LOG_RETENTION_DAYS {
                fs::remove_file(&path)
                    .context(format!("Failed to remove old log file: {:?}", path))?;
                deleted_count += 1;
            }
        }
    }

    Ok(deleted_count)
}

pub fn cleanup_old_usage_files() -> Result<usize> {
    let usage_dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(BIFROST_DIR)
        .join("usage");

    if !usage_dir.exists() {
        return Ok(0);
    }

    let mut deleted_count = 0;
    let cutoff = Local::now().date_naive() - chrono::Duration::days(90);

    for entry in fs::read_dir(&usage_dir)
        .context(format!("Failed to read usage directory: {:?}", usage_dir))?
    {
        let entry = entry?;
        let path = entry.path();

        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        if !name.ends_with(".jsonl") {
            continue;
        }

        let date_str = name.trim_end_matches(".jsonl");
        if let Ok(date) = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
            && date < cutoff
        {
            fs::remove_file(&path)
                .context(format!("Failed to remove old usage file: {:?}", path))?;
            deleted_count += 1;
        }
    }

    Ok(deleted_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_bifrost_dir() {
        let dir = get_bifrost_dir();
        assert!(dir.is_ok());
        assert!(dir.unwrap().ends_with(BIFROST_DIR));
    }

    #[test]
    fn test_get_today_log_path() {
        let path = get_today_log_path();
        assert!(path.is_ok());
        let path = path.unwrap();
        assert!(
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with(".log")
        );
    }
}
