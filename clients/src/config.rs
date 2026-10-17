/// Unified configuration module that reads from Anchor.toml
/// This ensures single source of truth for RPC URLs and other configuration
use serde::Deserialize;
use std::path::PathBuf;
use std::fs;
use std::collections::HashMap;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

#[derive(Debug, Deserialize)]
struct AnchorToml {
    provider: Provider,
    #[serde(default)]
    programs: Programs,
    #[serde(default)]
    tokens: Tokens,
}

#[derive(Debug, Deserialize, Default)]
struct Programs {
    #[serde(default)]
    testnet: HashMap<String, String>,
    #[serde(default)]
    mainnet: HashMap<String, String>,
}

#[derive(Debug, Deserialize, Default)]
struct Tokens {
    #[serde(default)]
    testnet: HashMap<String, String>,
    #[serde(default)]
    mainnet: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct Provider {
    cluster: String,
    wallet: String,
    #[serde(default = "default_program_env")]
    program_env: String,
}

fn default_program_env() -> String {
    "testnet".to_string()
}

/// Environment variable that overrides Anchor.toml's program_env (set by the deploy scripts,
/// so admin clients use the environment being deployed)
const PROGRAM_ENV_OVERRIDE: &str = "MEMO_PROGRAM_ENV";

/// program_env in effect: MEMO_PROGRAM_ENV if set, otherwise Anchor.toml's program_env
fn effective_program_env(config: &AnchorToml) -> String {
    match std::env::var(PROGRAM_ENV_OVERRIDE) {
        Ok(env) if !env.is_empty() => env,
        _ => config.provider.program_env.clone(),
    }
}

/// Get the RPC URL from Anchor.toml (the MEMO_RPC_URL environment variable overrides it,
/// e.g. to point the clients at a local validator)
/// This is the single source of truth for RPC configuration
pub fn get_rpc_url() -> String {
    if let Ok(url) = std::env::var("MEMO_RPC_URL") {
        if !url.is_empty() {
            println!("✓ Using RPC from MEMO_RPC_URL: {}", url);
            return url;
        }
    }


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

/// Get the wallet path from Anchor.toml (the MEMO_WALLET environment variable overrides it)
pub fn get_wallet_path() -> String {
    if let Ok(wallet) = std::env::var("MEMO_WALLET") {
        if !wallet.is_empty() {
            let expanded = shellexpand::tilde(&wallet).to_string();
            println!("✓ Using wallet from MEMO_WALLET: {}", expanded);
            return expanded;
        }
    }

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

/// Get the program environment from Anchor.toml
/// Returns "testnet" or "mainnet"
pub fn get_program_env() -> String {
    let anchor_toml_path = get_anchor_toml_path();
    
    match read_anchor_config(&anchor_toml_path) {
        Ok(config) => {
            let env = effective_program_env(&config);
            if env != "testnet" && env != "mainnet" {
                eprintln!("⚠️  Warning: Invalid program_env '{}'", env);
                eprintln!("   Valid values: 'testnet' or 'mainnet'");
                eprintln!("   Falling back to 'testnet'");
                "testnet".to_string()
            } else {
                println!("✓ Using program_env: {}", env);
                env
            }
        },
        Err(e) => {
            eprintln!("⚠️  Warning: Could not read Anchor.toml: {}", e);
            eprintln!("   Falling back to program_env: testnet");
            "testnet".to_string()
        }
    }
}

/// Path of an authority keypair (e.g. "deploy_admin", "memo_token_mint") for the current program_env:
/// ~/.config/solana/memo-token/<program_env>/authority/<name>-keypair.json
/// Keypairs are stored encrypted; this file exists only while `scripts/keys.sh unlock <env>` is in effect.
pub fn get_authority_keypair_path(name: &str) -> PathBuf {
    let home = std::env::var("HOME").expect("HOME environment variable not set");
    PathBuf::from(home)
        .join(".config/solana/memo-token")
        .join(get_program_env())
        .join("authority")
        .join(format!("{}-keypair.json", name))
}

/// Read an authority keypair, with a hint to unlock the keys when it is missing
pub fn read_authority_keypair(name: &str) -> Result<solana_sdk::signature::Keypair, String> {
    let path = get_authority_keypair_path(name);
    solana_sdk::signature::read_keypair_file(&path).map_err(|e| {
        if path.exists() {
            format!("Failed to read {}: {}", path.display(), e)
        } else {
            let env = get_program_env();
            format!(
                "{} not found. Unlock the {} keys first: ./scripts/keys.sh unlock {}",
                path.display(), env, env
            )
        }
    })
}

/// Get a specific program ID from Anchor.toml based on program_env
/// 
/// # Arguments
/// * `program_name` - Program name (supports both "memo_mint" and "memo-mint" formats)
/// 
/// # Returns
/// * `Ok(Pubkey)` - The program ID
/// * `Err(String)` - Error message if program not found or invalid
pub fn get_program_id(program_name: &str) -> Result<Pubkey, String> {
    let anchor_toml_path = get_anchor_toml_path();
    
    match read_anchor_config(&anchor_toml_path) {
        Ok(config) => {
            let env = &effective_program_env(&config);
            
            // Validate program_env
            if env != "testnet" && env != "mainnet" {
                return Err(format!("Invalid program_env '{}'. Must be 'testnet' or 'mainnet'", env));
            }
            
            let program_map = if env == "testnet" {
                &config.programs.testnet
            } else {
                &config.programs.mainnet
            };
            
            // Normalize program name: support both memo-mint and memo_mint
            let normalized_name = program_name.replace("-", "_");
            
            if let Some(program_id_str) = program_map.get(&normalized_name) {
                match Pubkey::from_str(program_id_str) {
                    Ok(pubkey) => {
                        println!("✓ Loaded {} Program ID ({}): {}", program_name, env, pubkey);
                        Ok(pubkey)
                    },
                    Err(e) => Err(format!("Invalid program ID for {}: {}", program_name, e))
                }
            } else {
                Err(format!("Program '{}' not found in Anchor.toml [programs.{}]", program_name, env))
            }
        },
        Err(e) => {
            Err(format!("Could not read Anchor.toml: {}", e))
        }
    }
}

/// Get all program IDs based on current program_env
/// 
/// # Returns
/// * HashMap of program names to their Pubkeys
pub fn get_all_program_ids() -> HashMap<String, Pubkey> {
    let anchor_toml_path = get_anchor_toml_path();
    let mut result = HashMap::new();
    
    if let Ok(config) = read_anchor_config(&anchor_toml_path) {
        let env = &effective_program_env(&config);
        
        if env != "testnet" && env != "mainnet" {
            eprintln!("⚠️  Warning: Invalid program_env '{}', using testnet", env);
            if let Some(programs) = Some(&config.programs.testnet) {
                for (name, id_str) in programs {
                    if let Ok(pubkey) = Pubkey::from_str(id_str) {
                        result.insert(name.clone(), pubkey);
                    }
                }
            }
        } else {
            let program_map = if env == "testnet" {
                &config.programs.testnet
            } else {
                &config.programs.mainnet
            };
            
            for (name, id_str) in program_map {
                if let Ok(pubkey) = Pubkey::from_str(id_str) {
                    result.insert(name.clone(), pubkey);
                }
            }
        }
    }
    
    result
}

/// Get token mint address by name based on current program_env
/// 
/// # Arguments
/// * `token_name` - Name of the token (e.g., "memo_token")
/// 
/// # Returns
/// The Pubkey of the token mint for the current environment
/// 
/// # Example
/// ```rust
/// let mint = get_token_mint("memo_token")?;
/// ```
pub fn get_token_mint(token_name: &str) -> Result<Pubkey, Box<dyn std::error::Error>> {
    let anchor_toml_path = get_anchor_toml_path();
    let config = read_anchor_config(&anchor_toml_path)?;
    let env = &effective_program_env(&config);
    
    // Validate program_env
    if env != "testnet" && env != "mainnet" {
        return Err(format!("Invalid program_env '{}'. Must be 'testnet' or 'mainnet'", env).into());
    }
    
    let token_map = if env == "testnet" {
        &config.tokens.testnet
    } else {
        &config.tokens.mainnet
    };
    
    // Support both underscore and dash formats
    let normalized_name = token_name.replace("-", "_");
    
    if let Some(mint_str) = token_map.get(&normalized_name).or_else(|| token_map.get(token_name)) {
        match Pubkey::from_str(mint_str) {
            Ok(pubkey) => {
                println!("✓ Loaded {} token mint ({}): {}", token_name, env, pubkey);
                Ok(pubkey)
            },
            Err(e) => Err(format!("Invalid token mint address for {}: {}", token_name, e).into())
        }
    } else {
        Err(format!("Token '{}' not found in Anchor.toml [tokens.{}]", token_name, env).into())
    }
}

/// Get all token mints for the current environment
/// 
/// # Returns
/// HashMap of token name -> Pubkey for all configured tokens
/// 
/// # Example
/// ```rust
/// let all_mints = get_all_token_mints()?;
/// for (name, mint) in all_mints {
///     println!("{}: {}", name, mint);
/// }
/// ```
pub fn get_all_token_mints() -> Result<std::collections::HashMap<String, Pubkey>, Box<dyn std::error::Error>> {
    let anchor_toml_path = get_anchor_toml_path();
    let config = read_anchor_config(&anchor_toml_path)?;
    let env = &effective_program_env(&config);
    
    // Validate program_env
    if env != "testnet" && env != "mainnet" {
        return Err(format!("Invalid program_env '{}'. Must be 'testnet' or 'mainnet'", env).into());
    }
    
    let token_map = if env == "testnet" {
        &config.tokens.testnet
    } else {
        &config.tokens.mainnet
    };
    
    let mut result = std::collections::HashMap::new();
    for (name, mint_str) in token_map {
        let mint = Pubkey::from_str(mint_str)?;
        result.insert(name.clone(), mint);
    }
    
    Ok(result)
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
    
    #[test]
    fn test_get_program_env() {
        let env = get_program_env();
        assert!(env == "testnet" || env == "mainnet");
    }
}

