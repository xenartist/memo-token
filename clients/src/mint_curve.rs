/// Client-side view of the memo-mint emission curve
/// Keep in sync with calculate_dynamic_mint_amount in programs/memo-mint/src/lib.rs

/// 1 token in lamports (decimals = 6)
pub const DECIMAL_FACTOR: u64 = 1_000_000;
/// Supply (tokens) where the curve starts stepping down
pub const CURVE_START_TOKENS: u64 = 10_000_000;
/// Number of decades from the curve start to the 10T max supply
pub const CURVE_DECADES: u32 = 6;

/// Expected mint amount (lamports) for a given supply (lamports) before the mint
pub fn expected_mint_amount(supply_lamports: u64) -> u64 {
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
    fn test_invalid_amounts() {
        assert!(!validate_mint_amount(0).0);
        assert!(!validate_mint_amount(123_456).0);
        assert!(!validate_mint_amount(950_000).0);
        assert!(!validate_mint_amount(2 * DECIMAL_FACTOR).0);
    }
}
