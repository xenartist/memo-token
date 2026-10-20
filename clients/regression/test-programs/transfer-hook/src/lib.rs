//! Minimal Token-2022 transfer hook, used by the memo-home regression suite (and earlier to test
//! wallet support on X1 testnet).
//!
//! On every transfer the hook updates a holder record PDA `[b"holder", mint]`
//! (an extra account the wallet must resolve). When the record is locked the
//! hook rejects the transfer, so wallets' error display can be checked too.
//!
//! Test-only: Execute does not verify that it is called by Token-2022.

use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint,
    entrypoint::ProgramResult,
    msg,
    program::invoke_signed,
    program_error::ProgramError,
    pubkey::Pubkey,
    rent::Rent,
    system_instruction, system_program,
    sysvar::Sysvar,
};
use spl_tlv_account_resolution::{account::ExtraAccountMeta, seeds::Seed, state::ExtraAccountMetaList};
use spl_transfer_hook_interface::{
    get_extra_account_metas_address_and_bump_seed,
    instruction::{ExecuteInstruction, TransferHookInstruction},
};

entrypoint!(process_instruction);

const HOLDER_SEED: &[u8] = b"holder";
/// authority (32) + last_holder (32) + transfer_count (8) + locked (1)
const HOLDER_RECORD_LEN: usize = 73;

const IX_SETUP: u8 = 0xA0;
const IX_SET_LOCK: u8 = 0xA1;

pub fn process_instruction(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    if let Ok(TransferHookInstruction::Execute { amount }) = TransferHookInstruction::unpack(data) {
        return execute(program_id, accounts, amount);
    }
    match data.first() {
        Some(&IX_SETUP) => setup(program_id, accounts),
        Some(&IX_SET_LOCK) => set_lock(program_id, accounts, data.get(1).copied().unwrap_or(0)),
        _ => Err(ProgramError::InvalidInstructionData),
    }
}

fn holder_record_address(mint: &Pubkey, program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[HOLDER_SEED, mint.as_ref()], program_id)
}

/// Reads the mint authority (COption<Pubkey> at offset 0 of the mint data).
fn mint_authority(mint: &AccountInfo) -> Result<Pubkey, ProgramError> {
    let data = mint.try_borrow_data()?;
    if data.len() < 36 || data[0..4] != [1, 0, 0, 0] {
        return Err(ProgramError::InvalidAccountData);
    }
    Ok(Pubkey::new_from_array(data[4..36].try_into().unwrap()))
}

fn create_pda<'a>(
    payer: &AccountInfo<'a>,
    target: &AccountInfo<'a>,
    system: &AccountInfo<'a>,
    program_id: &Pubkey,
    len: usize,
    seeds: &[&[u8]],
) -> ProgramResult {
    let lamports = Rent::get()?.minimum_balance(len);
    invoke_signed(
        &system_instruction::create_account(payer.key, target.key, lamports, len as u64, program_id),
        &[payer.clone(), target.clone(), system.clone()],
        &[seeds],
    )
}

/// Accounts: [payer+mint authority (s,w), mint, extra_metas (w), holder_record (w), system_program]
fn setup(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let it = &mut accounts.iter();
    let authority = next_account_info(it)?;
    let mint = next_account_info(it)?;
    let extra_metas = next_account_info(it)?;
    let holder_record = next_account_info(it)?;
    let system = next_account_info(it)?;

    if !authority.is_signer || mint_authority(mint)? != *authority.key || *system.key != system_program::ID {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let (metas_addr, metas_bump) = get_extra_account_metas_address_and_bump_seed(mint.key, program_id);
    let (record_addr, record_bump) = holder_record_address(mint.key, program_id);
    if *extra_metas.key != metas_addr || *holder_record.key != record_addr {
        return Err(ProgramError::InvalidSeeds);
    }

    // Execute account order: 0 source, 1 mint, 2 destination, 3 owner, 4 extra_metas, 5 holder_record
    let metas = [ExtraAccountMeta::new_with_seeds(
        &[Seed::Literal { bytes: HOLDER_SEED.to_vec() }, Seed::AccountKey { index: 1 }],
        false,
        true,
    )?];
    let metas_len = ExtraAccountMetaList::size_of(metas.len())?;
    create_pda(authority, extra_metas, system, program_id, metas_len,
        &[b"extra-account-metas", mint.key.as_ref(), &[metas_bump]])?;
    ExtraAccountMetaList::init::<ExecuteInstruction>(&mut extra_metas.try_borrow_mut_data()?, &metas)?;

    create_pda(authority, holder_record, system, program_id, HOLDER_RECORD_LEN,
        &[HOLDER_SEED, mint.key.as_ref(), &[record_bump]])?;
    holder_record.try_borrow_mut_data()?[0..32].copy_from_slice(authority.key.as_ref());
    msg!("hook setup done for mint {}", mint.key);
    Ok(())
}

/// Accounts: [authority (s), mint, holder_record (w)]
fn set_lock(program_id: &Pubkey, accounts: &[AccountInfo], locked: u8) -> ProgramResult {
    let it = &mut accounts.iter();
    let authority = next_account_info(it)?;
    let mint = next_account_info(it)?;
    let holder_record = next_account_info(it)?;
    if holder_record.owner != program_id || *holder_record.key != holder_record_address(mint.key, program_id).0 {
        return Err(ProgramError::InvalidSeeds);
    }
    let mut data = holder_record.try_borrow_mut_data()?;
    if !authority.is_signer || data[0..32] != authority.key.to_bytes() {
        return Err(ProgramError::MissingRequiredSignature);
    }
    data[72] = (locked != 0) as u8;
    msg!("holder record locked = {}", data[72]);
    Ok(())
}

fn execute(program_id: &Pubkey, accounts: &[AccountInfo], amount: u64) -> ProgramResult {
    let it = &mut accounts.iter();
    let _source = next_account_info(it)?;
    let mint = next_account_info(it)?;
    let destination = next_account_info(it)?;
    let _owner = next_account_info(it)?;
    let _extra_metas = next_account_info(it)?;
    let holder_record = next_account_info(it)?;

    if holder_record.owner != program_id || *holder_record.key != holder_record_address(mint.key, program_id).0 {
        return Err(ProgramError::InvalidSeeds);
    }
    let mut data = holder_record.try_borrow_mut_data()?;
    if data[72] != 0 {
        msg!("transfer rejected: NFT is locked");
        return Err(ProgramError::Custom(1));
    }
    // Token account layout: mint (0..32), owner (32..64)
    let new_holder: [u8; 32] = destination.try_borrow_data()?[32..64].try_into().unwrap();
    data[32..64].copy_from_slice(&new_holder);
    let count = u64::from_le_bytes(data[64..72].try_into().unwrap()).saturating_add(1);
    data[64..72].copy_from_slice(&count.to_le_bytes());
    msg!("hook: amount {} -> holder {} (transfer #{})", amount, Pubkey::new_from_array(new_holder), count);
    Ok(())
}
