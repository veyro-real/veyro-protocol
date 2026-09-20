use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
    instruction::{AccountMeta, Instruction},
    program::invoke_signed,
};
use solana_instructions_sysvar::{load_current_index_checked, load_instruction_at_checked};

declare_id!("2Z7xH99Z4YvG4U2Ew5PUZtVh8FE1VRhQ1Mo9dFvRvS3Q");
pub const MAX_RECIPIENTS: usize = 8;
pub const MAX_PROGRAMS: usize = 4;
pub const POLICY_VERSION: u8 = 2;
pub const LIVE_POLICY_VERSION: u8 = 1;
pub const TOKEN: Pubkey = pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
pub const COMPUTE_BUDGET: Pubkey = pubkey!("ComputeBudget111111111111111111111111111111");

#[program]
pub mod veyro {
    use super::*;
    // A deliberately fixed-price test-token pool, not a mainnet DEX router.
    pub fn create_pool(
        ctx: Context<CreatePool>,
        quote_mint: Pubkey,
        output_mint: Pubkey,
        rate: u64,
    ) -> Result<()> {
        require!(
            rate > 0 && rate <= 1_000_000 && quote_mint != output_mint,
            VeyroError::InvalidPolicy
        );
        let p = &mut ctx.accounts.pool;
        p.admin = ctx.accounts.admin.key();
        p.quote_mint = quote_mint;
        p.output_mint = output_mint;
        p.rate = rate;
        p.bump = ctx.bumps.pool;
        Ok(())
    }
    pub fn create_policy(
        ctx: Context<CreatePolicy>,
        agent: Pubkey,
        executor: Pubkey,
        allowed_recipients: Vec<Pubkey>,
        max_amount: u64,
        total_limit: u64,
        expires_at: i64,
        allowed_programs: Vec<Pubkey>,
        min_rate: u64,
    ) -> Result<()> {
        require!(
            max_amount > 0 && total_limit >= max_amount && min_rate > 0,
            VeyroError::InvalidPolicy
        );
        require!(
            expires_at > Clock::get()?.unix_timestamp,
            VeyroError::Expired
        );
        require!(
            agent != ctx.accounts.owner.key()
                && executor != agent
                && executor != ctx.accounts.owner.key(),
            VeyroError::InvalidPolicy
        );
        validate_allowlists(&allowed_recipients, &allowed_programs)?;
        let p = &mut ctx.accounts.policy;
        p.version = POLICY_VERSION;
        p.owner = ctx.accounts.owner.key();
        p.agent = agent;
        p.executor = executor;
        p.allowed_recipients = allowed_recipients;
        p.pool = ctx.accounts.pool.key();
        p.max_amount = max_amount;
        p.total_limit = total_limit;
        p.expires_at = expires_at;
        p.allowed_programs = allowed_programs;
        p.min_rate = min_rate;
        p.spent = 0;
        p.nonce = 0;
        p.active = true;
        p.bump = ctx.bumps.policy;
        emit!(PolicyCreated {
            policy: p.key(),
            owner: p.owner,
            agent
        });
        Ok(())
    }
    pub fn create_live_policy(
        ctx: Context<CreateLivePolicy>,
        agent: Pubkey,
        executor: Pubkey,
        allowed_recipients: Vec<Pubkey>,
        max_amount: u64,
        total_limit: u64,
        expires_at: i64,
        allowed_programs: Vec<Pubkey>,
    ) -> Result<()> {
        require!(
            max_amount > 0 && total_limit >= max_amount,
            VeyroError::InvalidPolicy
        );
        require!(
            expires_at > Clock::get()?.unix_timestamp,
            VeyroError::Expired
        );
        require!(
            agent != ctx.accounts.owner.key()
                && executor != agent
                && executor != ctx.accounts.owner.key(),
            VeyroError::InvalidPolicy
        );
        validate_allowlists(&allowed_recipients, &allowed_programs)?;
        require!(
            !allowed_recipients.is_empty() && !allowed_programs.is_empty(),
            VeyroError::InvalidPolicy
        );
        validate_mint(&ctx.accounts.quote_mint)?;
        let policy = &mut ctx.accounts.policy;
        policy.version = LIVE_POLICY_VERSION;
        policy.owner = ctx.accounts.owner.key();
        policy.agent = agent;
        policy.executor = executor;
        policy.quote_mint = ctx.accounts.quote_mint.key();
        policy.max_amount = max_amount;
        policy.total_limit = total_limit;
        policy.spent = 0;
        policy.expires_at = expires_at;
        policy.nonce = 0;
        policy.active = true;
        policy.bump = ctx.bumps.policy;
        policy.allowed_recipients = allowed_recipients;
        policy.allowed_programs = allowed_programs;
        emit!(PolicyCreated {
            policy: policy.key(),
            owner: policy.owner,
            agent
        });
        Ok(())
    }
    pub fn revoke_live(ctx: Context<ManageLive>) -> Result<()> {
        require!(
            ctx.accounts.policy.version == LIVE_POLICY_VERSION,
            VeyroError::InvalidPolicy
        );
        ctx.accounts.policy.active = false;
        emit!(Revoked {
            policy: ctx.accounts.policy.key(),
            at: Clock::get()?.unix_timestamp
        });
        Ok(())
    }
    pub fn execute_route<'info>(
        ctx: Context<'info, ExecuteRoute<'info>>,
        amount: u64,
        min_output: u64,
        nonce: u64,
        route_data: Vec<u8>,
    ) -> Result<()> {
        require!(
            anchor_lang::solana_program::instruction::get_stack_height() == 1,
            VeyroError::UnsupportedTransaction
        );
        validate_route_transaction(&ctx.accounts.instructions)?;
        require!(
            !route_data.is_empty() && route_data.len() <= 1_024,
            VeyroError::InvalidRoute
        );
        let policy_key = ctx.accounts.policy.key();
        let (owner, agent, bump, quote_mint) = {
            let policy = &ctx.accounts.policy;
            require!(
                policy.version == LIVE_POLICY_VERSION,
                VeyroError::InvalidPolicy
            );
            validate_spend(
                policy.active,
                Clock::get()?.unix_timestamp,
                policy.expires_at,
                nonce,
                policy.nonce,
                amount,
                policy.max_amount,
                policy.spent,
                policy.total_limit,
            )?;
            require!(
                policy
                    .allowed_programs
                    .contains(ctx.accounts.route_program.key),
                VeyroError::ProgramNotAllowed
            );
            (policy.owner, policy.agent, policy.bump, policy.quote_mint)
        };
        validate_token(&ctx.accounts.vault, &quote_mint, &policy_key)?;
        let recipient_authority = token_authority(&ctx.accounts.destination)?;
        require!(
            ctx.accounts
                .policy
                .allowed_recipients
                .contains(&recipient_authority),
            VeyroError::RecipientNotAllowed
        );
        validate_destination_token(&ctx.accounts.destination, &recipient_authority)?;
        require!(
            ctx.accounts.vault.key() != ctx.accounts.destination.key(),
            VeyroError::InvalidAccounts
        );
        let source_before = token_amount(&ctx.accounts.vault)?;
        let destination_before = token_amount(&ctx.accounts.destination)?;

        let route_accounts: Vec<AccountMeta> = ctx
            .remaining_accounts
            .iter()
            .map(|account| {
                if account.key() == policy_key {
                    AccountMeta::new_readonly(policy_key, true)
                } else if account.is_writable {
                    AccountMeta::new(account.key(), false)
                } else {
                    AccountMeta::new_readonly(account.key(), false)
                }
            })
            .collect();
        require!(
            route_accounts
                .iter()
                .any(|account| account.pubkey == policy_key && account.is_signer),
            VeyroError::InvalidRoute
        );
        require!(
            route_accounts
                .iter()
                .any(|account| account.pubkey == ctx.accounts.vault.key() && account.is_writable),
            VeyroError::InvalidRoute
        );
        require!(
            route_accounts
                .iter()
                .any(|account| account.pubkey == ctx.accounts.destination.key()
                    && account.is_writable),
            VeyroError::InvalidRoute
        );
        let route = Instruction {
            program_id: ctx.accounts.route_program.key(),
            accounts: route_accounts,
            data: route_data,
        };
        let mut account_infos: Vec<AccountInfo> = ctx.remaining_accounts.iter().cloned().collect();
        account_infos.push(ctx.accounts.policy.to_account_info());
        account_infos.push(ctx.accounts.route_program.to_account_info());
        let bump_seed = [bump];
        let seeds: &[&[u8]] = &[b"live-policy", owner.as_ref(), agent.as_ref(), &bump_seed];
        invoke_signed(&route, &account_infos, &[seeds])?;

        let source_after = token_amount(&ctx.accounts.vault)?;
        let destination_after = token_amount(&ctx.accounts.destination)?;
        let debited = source_before
            .checked_sub(source_after)
            .ok_or(VeyroError::InvalidRoute)?;
        let received = destination_after
            .checked_sub(destination_before)
            .ok_or(VeyroError::InvalidRoute)?;
        require!(
            debited > 0 && debited <= amount,
            VeyroError::RouteDebitMismatch
        );
        require!(
            received >= min_output && min_output > 0,
            VeyroError::MinimumOutputNotMet
        );
        let policy = &mut ctx.accounts.policy;
        policy.spent = policy
            .spent
            .checked_add(debited)
            .ok_or(VeyroError::Overflow)?;
        policy.nonce = policy.nonce.checked_add(1).ok_or(VeyroError::Overflow)?;
        emit!(RouteExecuted {
            policy: policy.key(),
            agent: policy.agent,
            route_program: ctx.accounts.route_program.key(),
            amount: debited,
            output: received,
            nonce,
            spent: policy.spent,
            at: Clock::get()?.unix_timestamp
        });
        Ok(())
    }
    pub fn recover_live(ctx: Context<RecoverLive>, amount: u64) -> Result<()> {
        let policy = &ctx.accounts.policy;
        require!(
            policy.version == LIVE_POLICY_VERSION,
            VeyroError::InvalidPolicy
        );
        require!(!policy.active, VeyroError::MustRevoke);
        validate_token(&ctx.accounts.vault, &policy.quote_mint, &policy.key())?;
        validate_token(&ctx.accounts.destination, &policy.quote_mint, &policy.owner)?;
        let bump = [policy.bump];
        let seeds: &[&[u8]] = &[
            b"live-policy",
            policy.owner.as_ref(),
            policy.agent.as_ref(),
            &bump,
        ];
        transfer(
            &ctx.accounts.token_program,
            &ctx.accounts.vault,
            &ctx.accounts.destination,
            &policy.to_account_info(),
            amount,
            seeds,
        )
    }
    pub fn revoke(ctx: Context<Manage>) -> Result<()> {
        require!(
            ctx.accounts.policy.version == POLICY_VERSION,
            VeyroError::InvalidPolicy
        );
        ctx.accounts.policy.active = false;
        emit!(Revoked {
            policy: ctx.accounts.policy.key(),
            at: Clock::get()?.unix_timestamp
        });
        Ok(())
    }
    pub fn execute_swap(
        ctx: Context<ExecuteSwap>,
        amount: u64,
        min_output: u64,
        nonce: u64,
    ) -> Result<()> {
        require!(
            anchor_lang::solana_program::instruction::get_stack_height() == 1,
            VeyroError::UnsupportedTransaction
        );
        let ix_info = ctx.accounts.instructions.to_account_info();
        require!(
            load_current_index_checked(&ix_info)? == 0,
            VeyroError::UnsupportedTransaction
        );
        let first = load_instruction_at_checked(0, &ix_info)?;
        require!(
            first.program_id == crate::ID && load_instruction_at_checked(1, &ix_info).is_err(),
            VeyroError::UnsupportedTransaction
        );
        // A wrapper program cannot pass the first.program_id check. This program never self-invokes.
        let p = &ctx.accounts.policy;
        require!(p.version == POLICY_VERSION, VeyroError::InvalidPolicy);
        let pool = &ctx.accounts.pool;
        validate_spend(
            p.active,
            Clock::get()?.unix_timestamp,
            p.expires_at,
            nonce,
            p.nonce,
            amount,
            p.max_amount,
            p.spent,
            p.total_limit,
        )?;
        require!(
            p.allowed_programs.contains(&TOKEN),
            VeyroError::ProgramNotAllowed
        );
        let output = amount.checked_mul(pool.rate).ok_or(VeyroError::Overflow)?;
        let floor = amount.checked_mul(p.min_rate).ok_or(VeyroError::Overflow)?;
        require!(
            min_output > 0 && min_output >= floor && output >= min_output,
            VeyroError::SlippageExceeded
        );
        validate_token(&ctx.accounts.vault, &pool.quote_mint, &p.key())?;
        validate_token(&ctx.accounts.pool_quote, &pool.quote_mint, &pool.key())?;
        validate_token(&ctx.accounts.pool_output, &pool.output_mint, &pool.key())?;
        let recipient_authority = token_authority(&ctx.accounts.recipient)?;
        require!(
            p.allowed_recipients.contains(&recipient_authority),
            VeyroError::RecipientNotAllowed
        );
        validate_token(
            &ctx.accounts.recipient,
            &pool.output_mint,
            &recipient_authority,
        )?;
        require!(
            ctx.accounts.vault.key() != ctx.accounts.pool_quote.key()
                && ctx.accounts.pool_output.key() != ctx.accounts.recipient.key(),
            VeyroError::InvalidAccounts
        );
        let owner = p.owner;
        let agent = p.agent;
        let bump = [p.bump];
        let policy_seeds: &[&[u8]] = &[b"policy", owner.as_ref(), agent.as_ref(), &bump];
        transfer(
            &ctx.accounts.token_program,
            &ctx.accounts.vault,
            &ctx.accounts.pool_quote,
            &p.to_account_info(),
            amount,
            policy_seeds,
        )?;
        let pool_bump = [pool.bump];
        let pool_seeds: &[&[u8]] = &[
            b"pool",
            pool.admin.as_ref(),
            pool.output_mint.as_ref(),
            &pool_bump,
        ];
        transfer(
            &ctx.accounts.token_program,
            &ctx.accounts.pool_output,
            &ctx.accounts.recipient,
            &pool.to_account_info(),
            output,
            pool_seeds,
        )?;
        let p = &mut ctx.accounts.policy;
        p.spent = p.spent.checked_add(amount).ok_or(VeyroError::Overflow)?;
        p.nonce = p.nonce.checked_add(1).ok_or(VeyroError::Overflow)?;
        emit!(Executed {
            policy: p.key(),
            agent: p.agent,
            amount,
            output,
            nonce,
            spent: p.spent,
            at: Clock::get()?.unix_timestamp
        });
        Ok(())
    }
    pub fn recover(ctx: Context<Recover>, amount: u64) -> Result<()> {
        let p = &ctx.accounts.policy;
        require!(p.version == POLICY_VERSION, VeyroError::InvalidPolicy);
        require!(!p.active, VeyroError::MustRevoke);
        validate_token(&ctx.accounts.vault, &ctx.accounts.pool.quote_mint, &p.key())?;
        validate_token(
            &ctx.accounts.destination,
            &ctx.accounts.pool.quote_mint,
            &p.owner,
        )?;
        let bump = [p.bump];
        let seeds: &[&[u8]] = &[b"policy", p.owner.as_ref(), p.agent.as_ref(), &bump];
        transfer(
            &ctx.accounts.token_program,
            &ctx.accounts.vault,
            &ctx.accounts.destination,
            &p.to_account_info(),
            amount,
            seeds,
        )
    }
}

fn validate_spend(
    active: bool,
    now: i64,
    expires: i64,
    nonce: u64,
    expected: u64,
    amount: u64,
    max: u64,
    spent: u64,
    limit: u64,
) -> Result<()> {
    require!(active, VeyroError::Revoked);
    require!(now < expires, VeyroError::Expired);
    require!(nonce == expected, VeyroError::StaleNonce);
    require!(amount > 0, VeyroError::InvalidAmount);
    require!(amount <= max, VeyroError::MaxTransactionExceeded);
    require!(
        spent.checked_add(amount).ok_or(VeyroError::Overflow)? <= limit,
        VeyroError::CumulativeLimitExceeded
    );
    Ok(())
}
fn validate_allowlists(recipients: &[Pubkey], programs: &[Pubkey]) -> Result<()> {
    require!(
        recipients.len() <= MAX_RECIPIENTS && programs.len() <= MAX_PROGRAMS,
        VeyroError::InvalidPolicy
    );
    for (index, recipient) in recipients.iter().enumerate() {
        require!(
            !recipients[..index].contains(recipient),
            VeyroError::InvalidPolicy
        );
    }
    for (index, program) in programs.iter().enumerate() {
        require!(
            !programs[..index].contains(program),
            VeyroError::InvalidPolicy
        );
    }
    Ok(())
}
fn validate_route_transaction(instructions: &AccountInfo) -> Result<()> {
    let current = load_current_index_checked(instructions)? as usize;
    let current_instruction = load_instruction_at_checked(current, instructions)?;
    require!(
        current_instruction.program_id == crate::ID,
        VeyroError::UnsupportedTransaction
    );
    for index in 0..current {
        require!(
            load_instruction_at_checked(index, instructions)?.program_id == COMPUTE_BUDGET,
            VeyroError::UnsupportedTransaction
        );
    }
    require!(
        load_instruction_at_checked(current + 1, instructions).is_err(),
        VeyroError::UnsupportedTransaction
    );
    Ok(())
}
fn validate_mint(info: &AccountInfo) -> Result<()> {
    require_keys_eq!(*info.owner, TOKEN, VeyroError::InvalidMint);
    let data = info.try_borrow_data()?;
    require!(data.len() == 82 && data[45] == 1, VeyroError::InvalidMint);
    Ok(())
}
fn token_authority(info: &AccountInfo) -> Result<Pubkey> {
    require_keys_eq!(*info.owner, TOKEN, VeyroError::InvalidAccounts);
    let data = info.try_borrow_data()?;
    require!(data.len() == 165, VeyroError::InvalidAccounts);
    let mut authority = [0u8; 32];
    authority.copy_from_slice(&data[32..64]);
    Ok(Pubkey::new_from_array(authority))
}
fn token_amount(info: &AccountInfo) -> Result<u64> {
    require_keys_eq!(*info.owner, TOKEN, VeyroError::InvalidAccounts);
    let data = info.try_borrow_data()?;
    require!(data.len() == 165, VeyroError::InvalidAccounts);
    Ok(u64::from_le_bytes(
        data[64..72]
            .try_into()
            .map_err(|_| VeyroError::InvalidAccounts)?,
    ))
}
fn validate_destination_token(info: &AccountInfo, authority: &Pubkey) -> Result<()> {
    require_keys_eq!(*info.owner, TOKEN, VeyroError::InvalidAccounts);
    let data = info.try_borrow_data()?;
    require!(
        data.len() == 165 && &data[32..64] == authority.as_ref() && data[108] == 1,
        VeyroError::RecipientNotAllowed
    );
    Ok(())
}
// Classic SPL Token only. Reject extensions, delegates, frozen/uninitialized/native accounts.
fn validate_token(info: &AccountInfo, mint: &Pubkey, authority: &Pubkey) -> Result<()> {
    require_keys_eq!(*info.owner, TOKEN, VeyroError::InvalidAccounts);
    let d = info.try_borrow_data()?;
    require!(
        d.len() == 165 && &d[0..32] == mint.as_ref() && &d[32..64] == authority.as_ref(),
        VeyroError::RecipientNotAllowed
    );
    require!(
        d[108] == 1 && d[72..76] == [0; 4] && d[109..113] == [0; 4] && d[129..133] == [0; 4],
        VeyroError::InvalidAccounts
    );
    Ok(())
}
fn transfer<'info>(
    token: &AccountInfo<'info>,
    source: &AccountInfo<'info>,
    dest: &AccountInfo<'info>,
    authority: &AccountInfo<'info>,
    amount: u64,
    seeds: &[&[u8]],
) -> Result<()> {
    let mut data = vec![3];
    data.extend_from_slice(&amount.to_le_bytes());
    let ix = Instruction {
        program_id: TOKEN,
        accounts: vec![
            AccountMeta::new(*source.key, false),
            AccountMeta::new(*dest.key, false),
            AccountMeta::new_readonly(*authority.key, true),
        ],
        data,
    };
    invoke_signed(
        &ix,
        &[
            source.clone(),
            dest.clone(),
            authority.clone(),
            token.clone(),
        ],
        &[seeds],
    )?;
    Ok(())
}
#[derive(Accounts)]
#[instruction(quote_mint: Pubkey, output_mint: Pubkey)]
pub struct CreatePool<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,
    #[account(init, payer=admin, space=8+32*3+8+1, seeds=[b"pool",admin.key().as_ref(),output_mint.as_ref()], bump)]
    pub pool: Account<'info, Pool>,
    pub system_program: Program<'info, System>,
}
#[derive(Accounts)]
#[instruction(agent: Pubkey)]
pub struct CreatePolicy<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    #[account(init, payer=owner, space=PolicyV2::SPACE, seeds=[b"policy",owner.key().as_ref(),agent.as_ref()], bump)]
    pub policy: Account<'info, PolicyV2>,
    pub pool: Account<'info, Pool>,
    pub system_program: Program<'info, System>,
}
#[derive(Accounts)]
pub struct Manage<'info> {
    pub owner: Signer<'info>,
    #[account(mut, has_one=owner, seeds=[b"policy",policy.owner.as_ref(),policy.agent.as_ref()],bump=policy.bump)]
    pub policy: Account<'info, PolicyV2>,
}
#[derive(Accounts)]
#[instruction(agent: Pubkey)]
pub struct CreateLivePolicy<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    #[account(init,payer=owner,space=LivePolicy::SPACE,seeds=[b"live-policy",owner.key().as_ref(),agent.as_ref()],bump)]
    pub policy: Account<'info, LivePolicy>,
    /// CHECK: validated as an initialized classic SPL mint and stored immutably.
    pub quote_mint: AccountInfo<'info>,
    pub system_program: Program<'info, System>,
}
#[derive(Accounts)]
pub struct ManageLive<'info> {
    pub owner: Signer<'info>,
    #[account(mut,has_one=owner,seeds=[b"live-policy",policy.owner.as_ref(),policy.agent.as_ref()],bump=policy.bump)]
    pub policy: Account<'info, LivePolicy>,
}
#[derive(Accounts)]
pub struct ExecuteRoute<'info> {
    pub agent: Signer<'info>,
    pub executor: Signer<'info>,
    #[account(mut,has_one=agent,has_one=executor,seeds=[b"live-policy",policy.owner.as_ref(),policy.agent.as_ref()],bump=policy.bump)]
    pub policy: Account<'info, LivePolicy>,
    /// CHECK: validated as a classic SPL account owned by the policy PDA with the policy quote mint.
    #[account(mut)]
    pub vault: AccountInfo<'info>,
    /// CHECK: validated as a classic SPL destination whose authority is allowlisted.
    #[account(mut)]
    pub destination: AccountInfo<'info>,
    /// CHECK: executable address must be in the owner-defined program allowlist.
    #[account(executable)]
    pub route_program: AccountInfo<'info>,
    /// CHECK: canonical SPL Token program.
    #[account(address=TOKEN,executable)]
    pub token_program: AccountInfo<'info>,
    /// CHECK: canonical instructions sysvar used to reject appended instructions.
    #[account(address=solana_instructions_sysvar::ID)]
    pub instructions: AccountInfo<'info>,
}
#[derive(Accounts)]
pub struct RecoverLive<'info> {
    pub owner: Signer<'info>,
    #[account(has_one=owner,seeds=[b"live-policy",policy.owner.as_ref(),policy.agent.as_ref()],bump=policy.bump)]
    pub policy: Account<'info, LivePolicy>,
    /// CHECK: validated as policy-owned quote token account.
    #[account(mut)]
    pub vault: AccountInfo<'info>,
    /// CHECK: validated as owner-owned quote token account.
    #[account(mut)]
    pub destination: AccountInfo<'info>,
    /// CHECK: canonical SPL Token program.
    #[account(address=TOKEN,executable)]
    pub token_program: AccountInfo<'info>,
}
#[derive(Accounts)]
pub struct ExecuteSwap<'info> {
    pub agent: Signer<'info>,
    pub executor: Signer<'info>,
    #[account(mut,has_one=agent,has_one=executor,has_one=pool,seeds=[b"policy",policy.owner.as_ref(),policy.agent.as_ref()],bump=policy.bump)]
    pub policy: Account<'info, PolicyV2>,
    #[account(seeds=[b"pool",pool.admin.as_ref(),pool.output_mint.as_ref()],bump=pool.bump)]
    pub pool: Account<'info, Pool>,
    /// CHECK: validated as classic SPL token account with policy authority and exact quote mint.
    #[account(mut)]
    pub vault: AccountInfo<'info>,
    /// CHECK: validated as pool-owned quote token account.
    #[account(mut)]
    pub pool_quote: AccountInfo<'info>,
    /// CHECK: validated as pool-owned output token account.
    #[account(mut)]
    pub pool_output: AccountInfo<'info>,
    /// CHECK: validated as output token account owned by an allowed recipient.
    #[account(mut)]
    pub recipient: AccountInfo<'info>,
    /// CHECK: fixed canonical SPL Token program.
    #[account(address=TOKEN, executable)]
    pub token_program: AccountInfo<'info>,
    /// CHECK: canonical instructions sysvar checked by the introspection functions.
    #[account(address=solana_instructions_sysvar::ID)]
    pub instructions: AccountInfo<'info>,
}
#[derive(Accounts)]
pub struct Recover<'info> {
    pub owner: Signer<'info>,
    #[account(has_one=owner,has_one=pool,seeds=[b"policy",policy.owner.as_ref(),policy.agent.as_ref()],bump=policy.bump)]
    pub policy: Account<'info, PolicyV2>,
    pub pool: Account<'info, Pool>,
    /// CHECK: token mint and authority validated in handler.
    #[account(mut)]
    pub vault: AccountInfo<'info>,
    /// CHECK: token mint and owner destination validated in handler.
    #[account(mut)]
    pub destination: AccountInfo<'info>,
    /// CHECK: canonical token program.
    #[account(address=TOKEN,executable)]
    pub token_program: AccountInfo<'info>,
}
#[account]
pub struct Pool {
    pub admin: Pubkey,
    pub quote_mint: Pubkey,
    pub output_mint: Pubkey,
    pub rate: u64,
    pub bump: u8,
}
#[account]
pub struct PolicyV2 {
    pub version: u8,
    pub owner: Pubkey,
    pub agent: Pubkey,
    pub executor: Pubkey,
    pub pool: Pubkey,
    pub max_amount: u64,
    pub total_limit: u64,
    pub spent: u64,
    pub expires_at: i64,
    pub nonce: u64,
    pub min_rate: u64,
    pub active: bool,
    pub bump: u8,
    pub allowed_recipients: Vec<Pubkey>,
    pub allowed_programs: Vec<Pubkey>,
}
impl PolicyV2 {
    pub const SPACE: usize =
        8 + 1 + 32 * 4 + 8 * 6 + 2 + 4 + 32 * MAX_RECIPIENTS + 4 + 32 * MAX_PROGRAMS;
}
#[account]
pub struct LivePolicy {
    pub version: u8,
    pub owner: Pubkey,
    pub agent: Pubkey,
    pub executor: Pubkey,
    pub quote_mint: Pubkey,
    pub max_amount: u64,
    pub total_limit: u64,
    pub spent: u64,
    pub expires_at: i64,
    pub nonce: u64,
    pub active: bool,
    pub bump: u8,
    pub allowed_recipients: Vec<Pubkey>,
    pub allowed_programs: Vec<Pubkey>,
}
impl LivePolicy {
    pub const SPACE: usize =
        8 + 1 + 32 * 4 + 8 * 5 + 2 + 4 + 32 * MAX_RECIPIENTS + 4 + 32 * MAX_PROGRAMS;
}
#[event]
pub struct PolicyCreated {
    pub policy: Pubkey,
    pub owner: Pubkey,
    pub agent: Pubkey,
}
#[event]
pub struct Revoked {
    pub policy: Pubkey,
    pub at: i64,
}
#[event]
pub struct Executed {
    pub policy: Pubkey,
    pub agent: Pubkey,
    pub amount: u64,
    pub output: u64,
    pub nonce: u64,
    pub spent: u64,
    pub at: i64,
}
#[event]
pub struct RouteExecuted {
    pub policy: Pubkey,
    pub agent: Pubkey,
    pub route_program: Pubkey,
    pub amount: u64,
    pub output: u64,
    pub nonce: u64,
    pub spent: u64,
    pub at: i64,
}
#[error_code]
pub enum VeyroError {
    #[msg("INVALID_POLICY")]
    InvalidPolicy,
    #[msg("REVOKED")]
    Revoked,
    #[msg("EXPIRED")]
    Expired,
    #[msg("STALE_NONCE")]
    StaleNonce,
    #[msg("INVALID_AMOUNT")]
    InvalidAmount,
    #[msg("MAX_TRANSACTION_EXCEEDED")]
    MaxTransactionExceeded,
    #[msg("CUMULATIVE_LIMIT_EXCEEDED")]
    CumulativeLimitExceeded,
    #[msg("PROGRAM_NOT_ALLOWED")]
    ProgramNotAllowed,
    #[msg("RECIPIENT_NOT_ALLOWED")]
    RecipientNotAllowed,
    #[msg("SLIPPAGE_EXCEEDED")]
    SlippageExceeded,
    #[msg("INVALID_ACCOUNTS")]
    InvalidAccounts,
    #[msg("UNSUPPORTED_TRANSACTION")]
    UnsupportedTransaction,
    #[msg("OVERFLOW")]
    Overflow,
    #[msg("MUST_REVOKE")]
    MustRevoke,
    #[msg("INVALID_ROUTE")]
    InvalidRoute,
    #[msg("ROUTE_DEBIT_MISMATCH")]
    RouteDebitMismatch,
    #[msg("MINIMUM_OUTPUT_NOT_MET")]
    MinimumOutputNotMet,
    #[msg("INVALID_MINT")]
    InvalidMint,
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn allowlists_are_bounded_unique_and_allow_empty() {
        let keys: Vec<Pubkey> = (0..9).map(|_| Pubkey::new_unique()).collect();
        assert!(validate_allowlists(&[], &[]).is_ok());
        assert!(validate_allowlists(&keys[..8], &keys[..4]).is_ok());
        assert!(validate_allowlists(&keys, &[]).is_err());
        assert!(validate_allowlists(&[], &keys[..5]).is_err());
        assert!(validate_allowlists(&[keys[0], keys[0]], &[]).is_err());
        assert!(validate_allowlists(&[], &[TOKEN, TOKEN]).is_err());
        assert_eq!(PolicyV2::SPACE, 579);
        assert_eq!(LivePolicy::SPACE, 571);
    }
    #[test]
    fn limits_and_boundaries() {
        assert!(validate_spend(true, 99, 100, 0, 0, 100, 100, 0, 150).is_ok());
        assert!(validate_spend(true, 100, 100, 0, 0, 1, 100, 0, 150).is_err());
        assert!(validate_spend(false, 1, 100, 0, 0, 1, 100, 0, 150).is_err());
        assert!(validate_spend(true, 1, 100, 1, 0, 1, 100, 0, 150).is_err());
        assert!(validate_spend(true, 1, 100, 0, 0, 101, 100, 0, 150).is_err());
        assert!(validate_spend(true, 1, 100, 0, 0, 51, 100, 100, 150).is_err());
        assert!(validate_spend(true, 1, 100, 0, 0, 50, 100, 100, 150).is_ok());
        assert!(validate_spend(true, 1, 100, 0, 0, 0, 100, 0, 150).is_err());
        assert!(validate_spend(true, 1, 100, 0, 0, u64::MAX, u64::MAX, 1, u64::MAX).is_err());
    }
}
