/// Client-side view of the memo-mint emission curve
/// Keep in sync with mint_amount_at_supply in programs/memo-mint/src/lib.rs
/// and min_burn_amount in the app programs (chat, project, profile, blog, forum)

/// 1 token in lamports (decimals = 6)
pub const DECIMAL_FACTOR: u64 = 1_000_000;
/// Supply (tokens) where the curve starts stepping down
pub const CURVE_START_TOKENS: u64 = 10_000_000;
/// Number of decades from the curve start to the 10T max supply
pub const CURVE_DECADES: u32 = 6;
/// Max supply in lamports (10T tokens)
pub const MAX_SUPPLY_LAMPORTS: u64 = 10_000_000_000_000 * DECIMAL_FACTOR;

/// Expected mint amount (lamports) for a given supply (lamports) before the mint.
/// At or above the max supply it returns the last step (1 lamport), like the program.
pub fn expected_mint_amount(supply_lamports: u64) -> u64 {
    if supply_lamports >= MAX_SUPPLY_LAMPORTS {
        return DECIMAL_FACTOR / 10u64.pow(CURVE_DECADES);
    }
    let start = CURVE_START_TOKENS * DECIMAL_FACTOR;
    if supply_lamports <= start {
        return DECIMAL_FACTOR;
    }
    // Steps take effect just above each round number
    let below = supply_lamports - 1;
    let mut magnitude = start;
    let mut decade_base = DECIMAL_FACTOR;
    while below / 10 >= magnitude {
        magnitude *= 10;
        decade_base /= 10;
    }
    let digit = below / magnitude;
    decade_base * (10 - digit) / 10
}

/// Minimum burn (lamports) for an app operation priced at `mints` times the current mint reward.
/// Same formula as min_burn_amount in the app programs, which use the supply before the burn.
pub fn min_burn_amount(mints: u64, supply_lamports: u64) -> u64 {
    mints.saturating_mul(expected_mint_amount(supply_lamports))
}

/// Fetch the current supply of `mint` and return the minimum burn (lamports) for `mints`.
/// The supply can move between this call and the transaction landing (burns lower it, which can
/// raise the reward by one step), so callers burning exactly the minimum may want a small buffer.
pub fn fetch_min_burn_amount(
    client: &solana_client::rpc_client::RpcClient,
    mint: &solana_sdk::pubkey::Pubkey,
    mints: u64,
) -> Result<u64, Box<dyn std::error::Error>> {
    // Confirmed rather than the client's default (often finalized), which can lag the latest mints
    let supply: u64 = client
        .get_token_supply_with_commitment(mint, solana_sdk::commitment_config::CommitmentConfig::confirmed())?
        .value
        .amount
        .parse()?;
    Ok(min_burn_amount(mints, supply))
}

/// Format lamports as a token amount with all 6 decimals
pub fn format_token_amount(lamports: u64) -> String {
    format!("{}.{:06}", lamports / DECIMAL_FACTOR, lamports % DECIMAL_FACTOR)
}

/// Parse a token amount with up to 6 decimals (e.g. "42069", "0.5", "12.000001") into lamports
pub fn parse_token_amount(s: &str) -> Option<u64> {
    let (whole, frac) = s.split_once('.').unwrap_or((s, ""));
    if (whole.is_empty() && frac.is_empty()) || frac.len() > 6
        || !whole.chars().chain(frac.chars()).all(|c| c.is_ascii_digit()) {
        return None;
    }
    let whole: u64 = if whole.is_empty() { 0 } else { whole.parse().ok()? };
    let frac: u64 = if frac.is_empty() { 0 } else { format!("{:0<6}", frac).parse().ok()? };
    whole.checked_mul(DECIMAL_FACTOR)?.checked_add(frac)
}

fn format_tokens(tokens: u64) -> String {
    match tokens {
        t if t >= 1_000_000_000_000 => format!("{}T", t / 1_000_000_000_000),
        t if t >= 1_000_000_000 => format!("{}B", t / 1_000_000_000),
        t => format!("{}M", t / 1_000_000),
    }
}

/// Check that a minted amount is one the curve can produce, and describe where it comes from
pub fn validate_mint_amount(raw_amount: u64) -> (bool, String) {
    if raw_amount == 0 {
        return (false, "No tokens minted - supply limit reached".to_string());
    }
    if raw_amount == DECIMAL_FACTOR {
        return (true, format!("1.0 token (supply up to {})", format_tokens(CURVE_START_TOKENS)));
    }

    let mut magnitude = CURVE_START_TOKENS;
    let mut decade_base = DECIMAL_FACTOR;
    for _ in 0..CURVE_DECADES {
        for digit in 1..=9u64 {
            if decade_base * (10 - digit) / 10 == raw_amount {
                return (
                    true,
                    format!(
                        "{:.6} token (supply above {} up to {})",
                        raw_amount as f64 / DECIMAL_FACTOR as f64,
                        format_tokens(digit * magnitude),
                        format_tokens((digit + 1) * magnitude)
                    ),
                );
            }
        }
        magnitude *= 10;
        decade_base /= 10;
    }

    (
        false,
        format!(
            "Unexpected amount: {} lamports ({:.6} tokens)",
            raw_amount,
            raw_amount as f64 / DECIMAL_FACTOR as f64
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_curve_matches_program_round_numbers() {
        assert_eq!(expected_mint_amount(0), 1_000_000);
        assert_eq!(expected_mint_amount(10_000_000 * DECIMAL_FACTOR), 1_000_000);
        assert_eq!(expected_mint_amount(10_000_000 * DECIMAL_FACTOR + 1), 900_000);
        assert_eq!(expected_mint_amount(90_000_000 * DECIMAL_FACTOR + 1), 100_000);
        assert_eq!(expected_mint_amount(100_000_000 * DECIMAL_FACTOR), 100_000);
        assert_eq!(expected_mint_amount(100_000_000 * DECIMAL_FACTOR + 1), 90_000);
        assert_eq!(expected_mint_amount(1_000_000_000 * DECIMAL_FACTOR + 1), 9_000);
        assert_eq!(expected_mint_amount(9_000_000_000_000 * DECIMAL_FACTOR + 1), 1);
    }

    #[test]
    fn test_every_curve_amount_is_valid() {
        let mut magnitude = CURVE_START_TOKENS;
        for _ in 0..CURVE_DECADES {
            for digit in 1..=9u64 {
                let amount = expected_mint_amount(digit * magnitude * DECIMAL_FACTOR + 1);
                assert!(validate_mint_amount(amount).0, "amount {} should be valid", amount);
            }
            magnitude *= 10;
        }
    }

    #[test]
    fn test_last_step_at_and_above_max_supply() {
        assert_eq!(expected_mint_amount(MAX_SUPPLY_LAMPORTS - 1), 1);
        assert_eq!(expected_mint_amount(MAX_SUPPLY_LAMPORTS), 1);
        assert_eq!(expected_mint_amount(u64::MAX), 1);
    }

    #[test]
    fn test_min_burn_amount_scales_with_mint_reward() {
        assert_eq!(min_burn_amount(42_069, 0), 42_069 * DECIMAL_FACTOR);
        assert_eq!(min_burn_amount(42_069, 10_000_000 * DECIMAL_FACTOR + 1), 42_069 * 900_000);
        assert_eq!(min_burn_amount(420, MAX_SUPPLY_LAMPORTS), 420);
        assert_eq!(format_token_amount(37_862_100_000), "37862.100000");
    }

    #[test]
    fn test_parse_token_amount() {
        assert_eq!(parse_token_amount("42069"), Some(42_069 * DECIMAL_FACTOR));
        assert_eq!(parse_token_amount("0.5"), Some(500_000));
        assert_eq!(parse_token_amount(".000001"), Some(1));
        assert_eq!(parse_token_amount("12.000001"), Some(12_000_001));
        assert_eq!(parse_token_amount("1.0000001"), None);
        assert_eq!(parse_token_amount("+5"), None);
        assert_eq!(parse_token_amount("."), None);
        assert_eq!(parse_token_amount("abc"), None);
        assert_eq!(parse_token_amount("99999999999999999999"), None);
    }

    #[test]
    fn test_invalid_amounts() {
        assert!(!validate_mint_amount(0).0);
        assert!(!validate_mint_amount(123_456).0);
        assert!(!validate_mint_amount(950_000).0);
        assert!(!validate_mint_amount(2 * DECIMAL_FACTOR).0);
    }
}
