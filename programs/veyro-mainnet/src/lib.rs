use solana_account_info::AccountInfo;
use solana_program_entrypoint::{entrypoint, ProgramResult};
use solana_program_error::ProgramError;
use solana_pubkey::Pubkey;

entrypoint!(process_instruction);

const CREATE_POLICY: u8 = 1;
const REVOKE_POLICY: u8 = 2;
const CHECK_SPEND: u8 = 3;
const VERSION: u8 = 1;

pub fn process_instruction(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let tag = *instruction_data.first().ok_or(ProgramError::InvalidInstructionData)?;
    match tag {
        CREATE_POLICY => create_policy(accounts, instruction_data),
        REVOKE_POLICY => revoke_policy(accounts),
        CHECK_SPEND => check_spend(accounts, instruction_data),
        _ => Err(ProgramError::InvalidInstructionData),
    }
}

fn create_policy(accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    if accounts.len() < 2 || !accounts[0].is_signer || !accounts[1].is_writable {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if data.len() != 1 + 32 + 8 + 8 + 8 + 8 + 32 + 32 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let mut policy = accounts[1].try_borrow_mut_data()?;
    if policy.len() < 154 {
        return Err(ProgramError::AccountDataTooSmall);
    }
    policy[0] = VERSION;
    policy[1] = 1;
    policy[2..34].copy_from_slice(accounts[0].key.as_ref());
    policy[34..66].copy_from_slice(&data[1..33]);
    policy[66..74].copy_from_slice(&data[33..41]);
    policy[74..82].copy_from_slice(&data[41..49]);
    policy[82..90].fill(0);
    policy[90..98].copy_from_slice(&data[49..57]);
    policy[98..106].copy_from_slice(&data[57..65]);
    policy[106..138].copy_from_slice(&data[65..97]);
    policy[138..154].copy_from_slice(&data[97..113]);
    Ok(())
}

fn revoke_policy(accounts: &[AccountInfo]) -> ProgramResult {
    if accounts.len() < 2 || !accounts[0].is_signer || !accounts[1].is_writable {
        return Err(ProgramError::MissingRequiredSignature);
    }
    let mut policy = accounts[1].try_borrow_mut_data()?;
    if policy.len() < 154 || policy[0] != VERSION || policy[2..34] != *accounts[0].key.as_ref() {
        return Err(ProgramError::InvalidAccountData);
    }
    policy[1] = 0;
    Ok(())
}

fn check_spend(accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    if accounts.len() < 2 || !accounts[0].is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if data.len() != 1 + 8 + 32 + 32 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let mut policy = accounts[1].try_borrow_mut_data()?;
    if policy.len() < 154 || policy[0] != VERSION || policy[1] != 1 {
        return Err(ProgramError::InvalidAccountData);
    }
    if policy[34..66] != *accounts[0].key.as_ref() {
        return Err(ProgramError::IncorrectAuthority);
    }
    let amount = read_u64(&data[1..9]);
    let max_amount = read_u64(&policy[66..74]);
    let total_limit = read_u64(&policy[74..82]);
    let spent = read_u64(&policy[82..90]);
    let expires_at = read_u64(&policy[90..98]);
    let now = read_u64(&data[9..17]);
    if amount == 0 || amount > max_amount || now > expires_at {
        return Err(ProgramError::InvalidArgument);
    }
    let next = spent.checked_add(amount).ok_or(ProgramError::ArithmeticOverflow)?;
    if next > total_limit {
        return Err(ProgramError::InsufficientFunds);
    }
    if policy[106..138] != data[17..49] || policy[138..154] != data[49..65] {
        return Err(ProgramError::InvalidArgument);
    }
    let nonce = read_u64(&policy[98..106])
        .checked_add(1)
        .ok_or(ProgramError::ArithmeticOverflow)?;
    policy[82..90].copy_from_slice(&next.to_le_bytes());
    policy[98..106].copy_from_slice(&nonce.to_le_bytes());
    Ok(())
}

fn read_u64(data: &[u8]) -> u64 {
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(data);
    u64::from_le_bytes(bytes)
}
