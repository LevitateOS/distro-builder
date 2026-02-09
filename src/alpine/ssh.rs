//! SSH configuration and host key generation.

use std::fs;
use std::process::Command;

use anyhow::{anyhow, Result};

use super::context::BuildContext;

/// Generate SSH host keys and configure sshd for live ISO.
///
/// # Arguments
/// * `ctx` - Build context
/// * `host_comment` - Comment for the key (e.g., "root@acornos" or "root@iuppiter")
pub fn setup_ssh(ctx: &BuildContext, host_comment: &str) -> Result<()> {
    let staging = &ctx.staging;
    let ssh_dir = staging.join("etc/ssh");

    // Ensure ssh directory exists
    fs::create_dir_all(&ssh_dir)?;

    // List of host key types to generate
    let key_types = vec![
        ("rsa", "ssh_host_rsa_key"),
        ("ecdsa", "ssh_host_ecdsa_key"),
        ("ed25519", "ssh_host_ed25519_key"),
    ];

    // Generate host keys using ssh-keygen from the host system
    // (not from staging, which doesn't have dependencies resolved yet)
    for (key_type, key_name) in &key_types {
        let key_path = ssh_dir.join(key_name);

        // Skip if key already exists
        if key_path.exists() {
            eprintln!("  \u{2713} SSH host key {} already exists", key_name);
            continue;
        }

        // Run ssh-keygen from the host to generate the key
        eprintln!("  Generating SSH {} host key...", key_type);
        let output = Command::new("ssh-keygen")
            .arg("-t")
            .arg(*key_type)
            .arg("-f")
            .arg(&key_path)
            .arg("-N")
            .arg("") // Empty passphrase
            .arg("-C")
            .arg(host_comment)
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!(
                "Failed to generate {} host key: {}",
                key_type,
                stderr
            ));
        }

        eprintln!("  \u{2713} Generated SSH {} host key", key_type);
    }

    // Configure sshd_config for live ISO (allow root login with password)
    configure_sshd(&ssh_dir)?;

    Ok(())
}

/// Configure sshd_config for the live ISO.
///
/// For the live ISO, we allow root login with password authentication
/// to make it easy for users to SSH in during testing.
fn configure_sshd(ssh_dir: &std::path::Path) -> Result<()> {
    let sshd_config_path = ssh_dir.join("sshd_config");

    // Read existing configuration
    let mut config = fs::read_to_string(&sshd_config_path).unwrap_or_else(|_| String::new());

    // Ensure critical settings are present and uncommented
    let critical_settings = vec![
        ("PermitRootLogin", "yes"),        // Allow root login for live ISO
        ("PasswordAuthentication", "yes"), // Allow password auth
        ("PubkeyAuthentication", "yes"),   // Also allow key auth
    ];

    for (setting, value) in critical_settings {
        // Check if the setting is active (uncommented) with the correct value.
        // We need to check line-by-line to avoid matching commented-out lines
        // (e.g., "#PermitRootLogin yes" contains "PermitRootLogin yes" as substring).
        let target = format!("{} {}", setting, value);
        let already_set = config.lines().any(|line| line.trim() == target);

        if !already_set {
            // Comment out any existing lines with this setting
            let lines: Vec<&str> = config.lines().collect();
            let mut new_lines = Vec::new();

            for line in lines {
                if line.starts_with(setting) {
                    new_lines.push(format!("#{}", line));
                } else {
                    new_lines.push(line.to_string());
                }
            }

            config = new_lines.join("\n");
            if !config.ends_with('\n') {
                config.push('\n');
            }

            // Add the new setting
            config.push_str(&format!("{} {}\n", setting, value));
        }
    }

    // Write updated configuration
    fs::write(&sshd_config_path, config)?;

    eprintln!("  \u{2713} Configured sshd_config for live ISO");

    Ok(())
}
