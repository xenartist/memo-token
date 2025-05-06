use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    signature::{read_keypair_file, Keypair, Signer},
    pubkey::Pubkey,
    transaction::Transaction,
};
use spl_token_2022::instruction as token_instruction;
use std::{str::FromStr, env, process};

// Token-2022 program ID constant
const TOKEN_2022_PROGRAM_ID: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";

fn main() {
    // Read command line arguments
    let args: Vec<String> = env::args().collect();
    
    if args.len() < 3 {
        println!("Usage: {} <mint_address_or_keypair> <program_id> [network_url]", args[0]);
        println!("  mint_address_or_keypair: Either a mint address or path to mint keypair file");
        println!("  program_id: The memo-token program ID");
        println!("  network_url: Optional network URL, defaults to testnet X1");
        return;
    }
    
    let mint_input = &args[1];
    let program_id_str = &args[2];
    
    // Use network URL from args or default to testnet X1
    let rpc_url = if args.len() > 3 {
        &args[3]
    } else {
        "https://rpc.testnet.x1.xyz"
    };
    
    println!("Connecting to network at: {}", rpc_url);
    let client = RpcClient::new_with_commitment(
        rpc_url.to_string(),
        solana_sdk::commitment_config::CommitmentConfig::confirmed(),
    );

    // Try to parse the input as either a pubkey or load it as a keypair file
    let mint_address = match Pubkey::from_str(mint_input) {
        Ok(pubkey) => {
            println!("Interpreted input as a mint public key: {}", pubkey);
            pubkey
        },
        Err(_) => {
            // Try loading it as a keypair file
            println!("Input is not a valid public key, trying to load as keypair file...");
            
            let expanded_path = shellexpand::tilde(mint_input).to_string();
            match read_keypair_file(&expanded_path) {
                Ok(keypair) => {
                    let pubkey = keypair.pubkey();
                    println!("Loaded keypair with public key: {}", pubkey);
                    pubkey
                },
                Err(e) => {
                    println!("Error: Could not interpret input as public key or keypair file.");
                    println!("If providing a public key, it should be a Base58 encoded string (typically 32-44 characters).");
                    println!("If providing a keypair file, make sure the path is correct.");
                    println!("Error details: {}", e);
                    process::exit(1);
                }
            }
        }
    };
    
    println!("Using token mint address: {}", mint_address);

    // Load payer keypair (wallet that will pay for transaction)
    let payer = read_keypair_file(
        shellexpand::tilde("~/.config/solana/id.json").to_string()
    ).expect("Failed to read payer keypair file");
    
    println!("Using payer: {}", payer.pubkey());

    // Parse program ID
    let program_id = match Pubkey::from_str(program_id_str) {
        Ok(pubkey) => pubkey,
        Err(e) => {
            println!("Error: Invalid program ID. Program ID should be a Base58 encoded string.");
            println!("Error details: {}", e);
            process::exit(1);
        }
    };
    
    println!("Program ID: {}", program_id);

    // Calculate PDA for mint authority
    let (mint_authority_pda, _bump) = Pubkey::find_program_address(
        &[b"mint_authority"],
        &program_id,
    );
    
    println!("Calculated mint authority PDA: {}", mint_authority_pda);

    // First, check if the mint actually exists and what type it is
    match client.get_account(&mint_address) {
        Ok(account) => {
            let owner = account.owner;
            let token_2022_id = Pubkey::from_str(TOKEN_2022_PROGRAM_ID).unwrap();
            
            println!("Mint account owner: {}", owner);
            
            // Check if it's a token-2022 or standard SPL token
            if owner == token_2022_id {
                println!("This is a Token-2022 token mint.");
                transfer_token_2022_authority(&client, &mint_address, &mint_authority_pda, &payer);
            } else if owner == spl_token::id() {
                println!("This is a standard SPL Token mint.");
                transfer_spl_token_authority(&client, &mint_address, &mint_authority_pda, &payer);
            } else {
                println!("Error: This address is not a valid token mint!");
                println!("Expected owner to be Token-2022 ({}) or standard SPL Token ({})",
                        token_2022_id, spl_token::id());
                println!("Actual owner: {}", owner);
                process::exit(1);
            }
        },
        Err(e) => {
            println!("Error: Could not find mint account. Make sure:");
            println!("1. The mint address is correct");
            println!("2. You are connected to the correct network");
            println!("3. The account exists on this network");
            println!("Error details: {}", e);
            process::exit(1);
        }
    }
}

fn transfer_token_2022_authority(
    client: &RpcClient,
    mint_address: &Pubkey,
    mint_authority_pda: &Pubkey,
    payer: &Keypair
) {
    let token_2022_id = Pubkey::from_str(TOKEN_2022_PROGRAM_ID).unwrap();

    // Create instruction to transfer mint authority
    let set_authority_ix = match spl_token_2022::instruction::set_authority(
        &token_2022_id,
        mint_address,
        Some(mint_authority_pda),
        spl_token_2022::instruction::AuthorityType::MintTokens,
        &payer.pubkey(),
        &[&payer.pubkey()],
    ) {
        Ok(ix) => ix,
        Err(e) => {
            println!("Error creating set_authority instruction: {}", e);
            println!("This could be because you don't have the right to transfer this mint's authority.");
            process::exit(1);
        }
    };
    
    // Get recent blockhash
    let recent_blockhash = client
        .get_latest_blockhash()
        .expect("Failed to get recent blockhash");
    
    // Create and sign transaction
    let transfer_auth_transaction = Transaction::new_signed_with_payer(
        &[set_authority_ix],
        Some(&payer.pubkey()),
        &[payer],
        recent_blockhash,
    );
    
    // Send and confirm transaction
    println!("\nTransferring mint authority to PDA using Token-2022 program...");
    match client.send_and_confirm_transaction_with_spinner(&transfer_auth_transaction) {
        Ok(sig) => {
            println!("\nMint authority transferred to PDA successfully!");
            println!("Transaction signature: {}", sig);
            println!("\nToken Info Summary:");
            println!("Mint address: {}", mint_address);
            println!("Mint authority (PDA): {}", mint_authority_pda);
            println!("\nSave these addresses for future use!");
            
            // Optional: Create a token account for the current wallet
            println!("\nTip: You can create a token account for your wallet using:");
            println!("spl-token create-account {}", mint_address);
        },
        Err(e) => {
            println!("Error transferring mint authority: {}", e);
            println!("Detailed error: {:?}", e);
            
            println!("\nYou can try using the spl-token CLI tool instead:");
            println!("spl-token set-authority {} mint {}", mint_address, mint_authority_pda);
        }
    }
}

fn transfer_spl_token_authority(
    client: &RpcClient,
    mint_address: &Pubkey,
    mint_authority_pda: &Pubkey,
    payer: &Keypair
) {
    // Create instruction to transfer mint authority
    let set_authority_ix = match spl_token::instruction::set_authority(
        &spl_token::id(),
        mint_address,
        Some(mint_authority_pda),
        spl_token::instruction::AuthorityType::MintTokens,
        &payer.pubkey(),
        &[&payer.pubkey()],
    ) {
        Ok(ix) => ix,
        Err(e) => {
            println!("Error creating set_authority instruction: {}", e);
            println!("This could be because you don't have the right to transfer this mint's authority.");
            process::exit(1);
        }
    };
    
    // Get recent blockhash
    let recent_blockhash = client
        .get_latest_blockhash()
        .expect("Failed to get recent blockhash");
    
    // Create and sign transaction
    let transfer_auth_transaction = Transaction::new_signed_with_payer(
        &[set_authority_ix],
        Some(&payer.pubkey()),
        &[payer],
        recent_blockhash,
    );
    
    // Send and confirm transaction
    println!("\nTransferring mint authority to PDA using standard SPL Token program...");
    match client.send_and_confirm_transaction_with_spinner(&transfer_auth_transaction) {
        Ok(sig) => {
            println!("\nMint authority transferred to PDA successfully!");
            println!("Transaction signature: {}", sig);
            println!("\nToken Info Summary:");
            println!("Mint address: {}", mint_address);
            println!("Mint authority (PDA): {}", mint_authority_pda);
            println!("\nSave these addresses for future use!");
        },
        Err(e) => {
            println!("Error transferring mint authority: {}", e);
            println!("Detailed error: {:?}", e);
            
            println!("\nYou can try using the spl-token CLI tool instead:");
            println!("spl-token set-authority {} mint {}", mint_address, mint_authority_pda);
        }
    }
}
