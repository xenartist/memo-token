/// Unified configuration module that reads from Anchor.toml
/// This ensures single source of truth for RPC URLs and other configuration
use serde::Deserialize;
use std::path::PathBuf;
use std::fs;

#[derive(Debug, Deserialize)]
struct AnchorToml {
    provider: Provider,
}

#[derive(Debug, Deserialize)]
struct Provider {
    cluster: String,
    wallet: String,
}

/// Get the RPC URL from Anchor.toml
/// This is the single source of truth for RPC configuration
pub fn get_rpc_url() -> String {
    // Get project root (assuming we're in clients/ subdirectory)
    let anchor_toml_path = get_anchor_toml_path();
    
    // Try to read and parse Anchor.toml
    match read_anchor_config(&anchor_toml_path) {
        Ok(config) => {
            println!("✓ Using RPC from Anchor.toml: {}", config.provider.cluster);
            config.provider.cluster
        },
        Err(e) => {
            eprintln!("⚠️  Warning: Could not read Anchor.toml: {}", e);
            eprintln!("   Falling back to default testnet RPC");
            "https://rpc.testnet.x1.xyz".to_string()
        }
    }
}

/// Get the wallet path from Anchor.toml
pub fn get_wallet_path() -> String {
    let anchor_toml_path = get_anchor_toml_path();
    
    match read_anchor_config(&anchor_toml_path) {
        Ok(config) => {
            let expanded = shellexpand::tilde(&config.provider.wallet).to_string();
            println!("✓ Using wallet from Anchor.toml: {}", expanded);
            expanded
        },
        Err(e) => {
            eprintln!("⚠️  Warning: Could not read Anchor.toml: {}", e);
            let default = format!("{}/.config/solana/id.json", std::env::var("HOME").unwrap_or_default());
            eprintln!("   Falling back to default wallet: {}", default);
            default
        }
    }
}

fn get_anchor_toml_path() -> PathBuf {
    // Try to find Anchor.toml by going up from current directory
    let mut path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    
    // Go up until we find Anchor.toml or reach root
    for _ in 0..5 {
        let anchor_toml = path.join("Anchor.toml");
        if anchor_toml.exists() {
            return anchor_toml;
        }
        if !path.pop() {
            break;
        }
    }
    
    // Fallback: assume we're in clients/ directory
    PathBuf::from("../Anchor.toml")
}

fn read_anchor_config(path: &PathBuf) -> Result<AnchorToml, Box<dyn std::error::Error>> {
    let contents = fs::read_to_string(path)?;
    let config: AnchorToml = toml::from_str(&contents)?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_rpc_url() {
        let url = get_rpc_url();
        assert!(!url.is_empty());
        assert!(url.starts_with("http"));
    }
}

