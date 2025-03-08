use std::str::FromStr;

use solana_sdk::pubkey::Pubkey;
use anchor_client::{solana_sdk::{
    commitment_config::CommitmentConfig, signature::read_keypair_file,
}, Client};
use anchor_client::solana_sdk::signature::Signer;
use anchor_spl::token::spl_token;
use spl_associated_token_account::{get_associated_token_address,create_associated_token_account};

fn main() {
    let rpc_url = "https://rpc.testnet.x1.xyz";
    let program_id = "TD8dwXKKg7M3QpWa9mQQpcvzaRasDU1MjmQWqZ9UZiw";
    let mint =  Pubkey::from_str("CrfhYtP7XtqFyHTWMyXp25CCzhjhzojngrPCZJ7RarUz").unwrap();

    let payer = read_keypair_file(
        shellexpand::tilde("~/.config/solana/id.json").to_string()
    ).expect("Failed to read keypair file");


    let client = Client::new_with_options(rpc_url.parse().unwrap(), &payer, CommitmentConfig::confirmed());
    let program_id = Pubkey::from_str(program_id).unwrap();
    let program = client.program(program_id).unwrap();
    let (mint_authority_pda, _bump) = Pubkey::find_program_address(
        &[b"mint_authority"],
        &program_id,
    );
    // Get user's token account
    let token_account = get_associated_token_address(
        &payer.pubkey(),
        &mint,
    );

    let token_account = spl_associated_token_account::get_associated_token_address(
        &payer.pubkey(),
        &mint,
    );

    let rpc = program.rpc();
    let mut tx_builder = program.request();
    if rpc.get_account(&token_account).is_err() {
        let create_token_account_ix = create_associated_token_account(
            &payer.pubkey(),
            &payer.pubkey(),
            &mint,
        );
        tx_builder = tx_builder.instruction(create_token_account_ix);
    }



    let tx = tx_builder
        .accounts(memo_token::accounts::ProcessTransfer {
            user: payer.pubkey(),
            mint:  mint,
            mint_authority: mint_authority_pda,
            token_account: token_account,
            token_program: spl_token::id(),
        })
        .args(memo_token::instruction::ProcessTransfer {

        })
        .send()
        .expect("");
    println!("Your transaction signature {}", tx);
}
