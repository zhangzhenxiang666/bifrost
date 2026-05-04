use anyhow::Result;
use flate2::read::GzDecoder;
use std::fs;
use std::path::{Path, PathBuf};
use sysinfo::Pid;
use tar::Archive;

use super::printing::{print_info, print_warning};
use super::start::cmd_start_internal;
use super::utils::{force_kill_process, get_stored_pid, is_process_running, is_server_running};
use crate::config::get_pid_file_path;

const GITHUB_REPO: &str = "zhangzhenxiang666/bifrost";

struct Platform {
    suffix: String,
}

impl Platform {
    fn detect() -> Self {
        #[cfg(windows)]
        {
            let arch = match std::env::consts::ARCH {
                "x86_64" => "amd64",
                "aarch64" => "aarch64",
                other => other,
            };
            Platform {
                suffix: format!("windows-{}", arch),
            }
        }

        #[cfg(not(windows))]
        {
            let os = std::process::Command::new("uname")
                .arg("-s")
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_else(|_| "Linux".to_string());

            let arch = std::process::Command::new("uname")
                .arg("-m")
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_else(|_| "x86_64".to_string());

            let suffix = match (os.as_str(), arch.as_str()) {
                ("Linux", "x86_64") => "linux-amd64",
                ("Linux", "aarch64") | ("Linux", "arm64") => "linux-aarch64",
                ("Darwin", "x86_64") => "darwin-amd64",
                ("Darwin", "arm64") => "darwin-aarch64",
                _ => "linux-amd64",
            };

            Platform {
                suffix: suffix.to_string(),
            }
        }
    }
}

fn get_server_binary_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(crate::config::BIFROST_DIR)
        .join("bin")
        .join(format!("bifrost-server{}", std::env::consts::EXE_SUFFIX))
}

fn fetch_remote_version() -> Result<String> {
    let url = format!(
        "https://api.github.com/repos/{}/releases/latest",
        GITHUB_REPO
    );
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let resp = client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "bifrost-upgrade/1.0")
        .send()?;
    let json: serde_json::Value = resp.json()?;
    let tag = json["tag_name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("failed to parse tag_name"))?;
    Ok(tag.to_string())
}

fn get_local_version() -> Result<semver::Version> {
    let binary_path =
        get_server_binary_path().with_file_name(format!("bifrost{}", std::env::consts::EXE_SUFFIX));
    let output = std::process::Command::new(&binary_path)
        .arg("-V")
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let version_str = stdout.trim().trim_start_matches("bifrost ");
    semver::Version::parse(version_str)
        .map_err(|_| anyhow::anyhow!("failed to parse version from: {}", version_str))
}

fn download_and_extract(github_tag: &str, platform: &Platform) -> Result<PathBuf> {
    let asset_name = format!("bifrost-{}-{}.tar.gz", github_tag, platform.suffix);
    let download_url = format!(
        "https://github.com/{}/releases/download/{}/{}",
        GITHUB_REPO, github_tag, asset_name
    );

    println!("Downloading: {}", asset_name);

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()?;
    let mut resp = client.get(&download_url).send()?;
    resp.error_for_status_ref()?;

    let temp_dir = std::env::temp_dir().join(format!("bifrost-upgrade-{}", std::process::id()));
    std::fs::create_dir_all(&temp_dir)?;

    let archive_path = temp_dir.join(&asset_name);
    let mut file = std::fs::File::create(&archive_path)?;
    std::io::copy(&mut resp, &mut file)?;

    println!("Extracting...");
    let tar_gz = std::fs::File::open(&archive_path)?;
    let decoder = GzDecoder::new(tar_gz);
    let mut archive = Archive::new(decoder);
    archive.unpack(&temp_dir)?;

    Ok(temp_dir)
}

#[cfg(windows)]
/// Replace a binary file, handling Windows file-locking safely.
///
/// On Windows, a running executable cannot be overwritten in-place, but it CAN be
/// renamed. We rename the old file to a `.old` backup first, then place the new one.
fn replace_binary(src: &Path, dst: &Path) -> Result<()> {
    if dst.exists() {
        let backup = dst.with_extension("old");
        let _ = std::fs::rename(dst, &backup);
    }
    std::fs::rename(src, dst)
        .or_else(|_| std::fs::copy(src, dst).and_then(|_| std::fs::remove_file(src)))?;
    Ok(())
}

fn install_binaries(temp_dir: &PathBuf, platform: &Platform) -> Result<()> {
    let server_binary_path = get_server_binary_path();
    let install_dir = server_binary_path.parent().unwrap();
    std::fs::create_dir_all(install_dir)?;

    let ext = std::env::consts::EXE_SUFFIX;
    let bifrost_src = temp_dir.join(format!("bifrost-{}{}", platform.suffix, ext));
    let server_src = temp_dir.join(format!("bifrost-server-{}{}", platform.suffix, ext));
    let bifrost_dst = install_dir.join(format!("bifrost{}", ext));
    let server_dst = install_dir.join(format!("bifrost-server{}", ext));

    #[cfg(windows)]
    {
        replace_binary(&bifrost_src, &bifrost_dst)?;
        replace_binary(&server_src, &server_dst)?;
    }

    #[cfg(not(windows))]
    {
        std::fs::rename(&bifrost_src, &bifrost_dst).or_else(|_| {
            std::fs::copy(&bifrost_src, &bifrost_dst)
                .and_then(|_| std::fs::remove_file(&bifrost_src))
        })?;
        std::fs::rename(&server_src, &server_dst).or_else(|_| {
            std::fs::copy(&server_src, &server_dst).and_then(|_| std::fs::remove_file(&server_src))
        })?;
    }

    std::fs::remove_dir_all(temp_dir).ok();

    println!("Installing binaries... Done");

    Ok(())
}

pub fn cmd_upgrade() -> Result<()> {
    println!();

    let platform = Platform::detect();

    let remote_tag = fetch_remote_version()?;
    let remote_tag_stripped = remote_tag.trim_start_matches('v');
    let remote = semver::Version::parse(remote_tag_stripped)
        .map_err(|_| anyhow::anyhow!("failed to parse remote version: {}", remote_tag_stripped))?;

    let local = match get_local_version() {
        Ok(v) => v,
        Err(e) => {
            print_warning(&format!(
                "Failed to get local version: {}. Proceeding with upgrade anyway.",
                e
            ));
            semver::Version::new(0, 0, 0)
        }
    };

    if local >= remote {
        println!("✓ Already up to date (v{})", local);
        println!();
        return Ok(());
    }

    println!("Checking version: v{} < v{} (remote)", local, remote);

    let temp_dir = download_and_extract(&remote_tag, &platform)?;

    let server_was_running = is_server_running();

    if server_was_running {
        print_info("Stopping service...", "");
        if let Some(pid) = get_stored_pid() {
            use sysinfo::System;
            let mut system = System::new();
            system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
            if let Some(process) = system.process(Pid::from_u32(pid)) {
                process.kill();
            }
            let mut attempts = 0;
            while is_process_running(pid) && attempts < 10 {
                std::thread::sleep(std::time::Duration::from_millis(500));
                attempts += 1;
            }
            if is_process_running(pid) {
                force_kill_process(pid);
            }
            if let Ok(pid_file) = get_pid_file_path() {
                fs::remove_file(&pid_file).ok();
            }
        }
        println!("Done");
    } else {
        print_info("Stopping service...", "Not running, skipping");
    }

    if let Err(e) = install_binaries(&temp_dir, &platform) {
        return Err(anyhow::anyhow!("Install failed: {}", e));
    }

    if server_was_running {
        print_info("Restarting service...", "");
        cmd_start_internal()?;
        println!("Done");
    }

    println!("✓ Upgrade complete (v{} → v{})", local, remote);
    println!();

    Ok(())
}
