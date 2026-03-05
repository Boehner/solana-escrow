// solana-escrow: Trustless Escrow Program for Solana
//
// Rebuilds a traditional Web2 escrow service as an on-chain Solana program.
// In Web2, an escrow service holds funds in a database and releases them
// when both parties agree. Here, the Solana runtime IS the escrow agent —
// funds are held in a PDA (Program Derived Address) and released by
// on-chain instruction logic. No trusted third party required.

#[allow(unused_variables)]
use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint,
    entrypoint::ProgramResult,
    msg,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    pubkey::Pubkey,
    rent::Rent,
    system_instruction,
    sysvar::Sysvar,
};

mod error;
mod instruction;
mod state;

use instruction::EscrowInstruction;
use state::{Escrow, EscrowStatus};

entrypoint!(process_instruction);

pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let instruction = EscrowInstruction::try_from_slice(instruction_data)
        .map_err(|_| ProgramError::InvalidInstructionData)?;

    match instruction {
        EscrowInstruction::Initialize { amount, description } => {
            process_initialize(program_id, accounts, amount, description)
        }
        EscrowInstruction::Fund => process_fund(program_id, accounts),
        EscrowInstruction::Release => process_release(program_id, accounts),
        EscrowInstruction::Dispute => process_dispute(program_id, accounts),
        EscrowInstruction::Resolve { release_to_recipient } => {
            process_resolve(program_id, accounts, release_to_recipient)
        }
        EscrowInstruction::Cancel => process_cancel(program_id, accounts),
    }
}

/// Initialize a new escrow agreement between a depositor and recipient.
/// Creates an escrow state account (PDA) and a vault (PDA) to hold funds.
fn process_initialize(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    amount: u64,
    description: [u8; 32],
) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();

    let depositor = next_account_info(accounts_iter)?;     // signer, pays for escrow
    let recipient = next_account_info(accounts_iter)?;      // receives funds on release
    let escrow_account = next_account_info(accounts_iter)?; // PDA: escrow state
    let vault = next_account_info(accounts_iter)?;          // PDA: holds lamports
    let system_program = next_account_info(accounts_iter)?;

    if !depositor.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    if amount == 0 {
        msg!("Error: escrow amount must be > 0");
        return Err(ProgramError::InvalidArgument);
    }

    // Derive escrow PDA
    let (escrow_pda, escrow_bump) = Pubkey::find_program_address(
        &[b"escrow", depositor.key.as_ref(), recipient.key.as_ref()],
        program_id,
    );
    if escrow_pda != *escrow_account.key {
        msg!("Error: escrow PDA mismatch");
        return Err(ProgramError::InvalidSeeds);
    }

    // Derive vault PDA
    let (vault_pda, vault_bump) = Pubkey::find_program_address(
        &[b"vault", escrow_account.key.as_ref()],
        program_id,
    );
    if vault_pda != *vault.key {
        msg!("Error: vault PDA mismatch");
        return Err(ProgramError::InvalidSeeds);
    }

    // Create escrow state account
    let escrow_data = Escrow {
        depositor: *depositor.key,
        recipient: *recipient.key,
        amount,
        status: EscrowStatus::Initialized,
        description,
        escrow_bump,
        vault_bump,
    };
    let serialized = borsh::to_vec(&escrow_data).map_err(|_| ProgramError::BorshIoError("serialize".to_string()))?;
    let space = serialized.len();
    let rent = Rent::get()?;
    let lamports = rent.minimum_balance(space);

    invoke_signed(
        &system_instruction::create_account(
            depositor.key,
            escrow_account.key,
            lamports,
            space as u64,
            program_id,
        ),
        &[depositor.clone(), escrow_account.clone(), system_program.clone()],
        &[&[b"escrow", depositor.key.as_ref(), recipient.key.as_ref(), &[escrow_bump]]],
    )?;

    // Write state
    escrow_data.serialize(&mut &mut escrow_account.data.borrow_mut()[..])?;

    // Create vault account (zero-data, just holds lamports)
    invoke_signed(
        &system_instruction::create_account(
            depositor.key,
            vault.key,
            0,  // will be funded separately
            0,
            program_id,
        ),
        &[depositor.clone(), vault.clone(), system_program.clone()],
        &[&[b"vault", escrow_account.key.as_ref(), &[vault_bump]]],
    )?;

    msg!("Escrow initialized: {} lamports from {} to {}", amount, depositor.key, recipient.key);
    Ok(())
}

/// Fund the escrow vault with the agreed amount.
fn process_fund(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();

    let depositor = next_account_info(accounts_iter)?;
    let escrow_account = next_account_info(accounts_iter)?;
    let vault = next_account_info(accounts_iter)?;
    let system_program = next_account_info(accounts_iter)?;

    if !depositor.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let mut escrow = Escrow::try_from_slice(&escrow_account.data.borrow())
        .map_err(|_| ProgramError::InvalidAccountData)?;

    if escrow.depositor != *depositor.key {
        msg!("Error: only depositor can fund");
        return Err(ProgramError::IllegalOwner);
    }

    if escrow.status != EscrowStatus::Initialized {
        msg!("Error: escrow not in Initialized state");
        return Err(ProgramError::InvalidAccountData);
    }

    // Verify vault PDA
    let (vault_pda, _) = Pubkey::find_program_address(
        &[b"vault", escrow_account.key.as_ref()],
        program_id,
    );
    if vault_pda != *vault.key {
        return Err(ProgramError::InvalidSeeds);
    }

    // Transfer lamports to vault
    invoke(
        &system_instruction::transfer(depositor.key, vault.key, escrow.amount),
        &[depositor.clone(), vault.clone(), system_program.clone()],
    )?;

    escrow.status = EscrowStatus::Funded;
    escrow.serialize(&mut &mut escrow_account.data.borrow_mut()[..])?;

    msg!("Escrow funded with {} lamports", escrow.amount);
    Ok(())
}

/// Release funds to the recipient. Only the depositor can authorize release.
fn process_release(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();

    let depositor = next_account_info(accounts_iter)?;
    let recipient = next_account_info(accounts_iter)?;
    let escrow_account = next_account_info(accounts_iter)?;
    let vault = next_account_info(accounts_iter)?;
    let system_program = next_account_info(accounts_iter)?;

    if !depositor.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let mut escrow = Escrow::try_from_slice(&escrow_account.data.borrow())
        .map_err(|_| ProgramError::InvalidAccountData)?;

    if escrow.depositor != *depositor.key {
        return Err(ProgramError::IllegalOwner);
    }
    if escrow.recipient != *recipient.key {
        return Err(ProgramError::InvalidArgument);
    }
    if escrow.status != EscrowStatus::Funded {
        msg!("Error: escrow not funded");
        return Err(ProgramError::InvalidAccountData);
    }

    // Transfer from vault to recipient using PDA-signed system transfer
    let vault_lamports = escrow.amount;
    invoke_signed(
        &system_instruction::transfer(vault.key, recipient.key, vault_lamports),
        &[vault.clone(), recipient.clone(), system_program.clone()],
        &[&[b"vault", escrow_account.key.as_ref(), &[escrow.vault_bump]]],
    )?;

    escrow.status = EscrowStatus::Released;
    escrow.serialize(&mut &mut escrow_account.data.borrow_mut()[..])?;

    msg!("Escrow released: {} lamports to {}", vault_lamports, recipient.key);
    Ok(())
}

/// Raise a dispute. Either depositor or recipient can dispute.
fn process_dispute(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();

    let disputer = next_account_info(accounts_iter)?;
    let escrow_account = next_account_info(accounts_iter)?;

    if !disputer.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let mut escrow = Escrow::try_from_slice(&escrow_account.data.borrow())
        .map_err(|_| ProgramError::InvalidAccountData)?;

    if escrow.depositor != *disputer.key && escrow.recipient != *disputer.key {
        msg!("Error: only depositor or recipient can dispute");
        return Err(ProgramError::IllegalOwner);
    }

    if escrow.status != EscrowStatus::Funded {
        msg!("Error: can only dispute funded escrows");
        return Err(ProgramError::InvalidAccountData);
    }

    escrow.status = EscrowStatus::Disputed;
    escrow.serialize(&mut &mut escrow_account.data.borrow_mut()[..])?;

    msg!("Escrow disputed by {}", disputer.key);
    Ok(())
}

/// Resolve a dispute. In this simplified model, the depositor acts as arbiter
/// (in production, this would be a separate arbiter keypair).
fn process_resolve(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    release_to_recipient: bool,
) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();

    let arbiter = next_account_info(accounts_iter)?;   // depositor acts as arbiter
    let depositor = next_account_info(accounts_iter)?;
    let recipient = next_account_info(accounts_iter)?;
    let escrow_account = next_account_info(accounts_iter)?;
    let vault = next_account_info(accounts_iter)?;
    let system_program = next_account_info(accounts_iter)?;

    if !arbiter.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let mut escrow = Escrow::try_from_slice(&escrow_account.data.borrow())
        .map_err(|_| ProgramError::InvalidAccountData)?;

    // For simplicity, depositor is the arbiter. A real implementation
    // would have a third-party arbiter field in the Escrow struct.
    if escrow.depositor != *arbiter.key {
        return Err(ProgramError::IllegalOwner);
    }

    if escrow.status != EscrowStatus::Disputed {
        msg!("Error: escrow not disputed");
        return Err(ProgramError::InvalidAccountData);
    }

    let vault_lamports = escrow.amount;
    let target = if release_to_recipient { recipient } else { depositor };

    invoke_signed(
        &system_instruction::transfer(vault.key, target.key, vault_lamports),
        &[vault.clone(), target.clone(), system_program.clone()],
        &[&[b"vault", escrow_account.key.as_ref(), &[escrow.vault_bump]]],
    )?;

    escrow.status = if release_to_recipient {
        EscrowStatus::Released
    } else {
        EscrowStatus::Cancelled
    };
    escrow.serialize(&mut &mut escrow_account.data.borrow_mut()[..])?;

    msg!("Dispute resolved: {} lamports to {}", vault_lamports, target.key);
    Ok(())
}

/// Cancel an unfunded escrow. Only the depositor can cancel.
fn process_cancel(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();

    let depositor = next_account_info(accounts_iter)?;
    let escrow_account = next_account_info(accounts_iter)?;
    let vault = next_account_info(accounts_iter)?;

    if !depositor.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let mut escrow = Escrow::try_from_slice(&escrow_account.data.borrow())
        .map_err(|_| ProgramError::InvalidAccountData)?;

    if escrow.depositor != *depositor.key {
        return Err(ProgramError::IllegalOwner);
    }

    if escrow.status != EscrowStatus::Initialized {
        msg!("Error: can only cancel unfunded escrows");
        return Err(ProgramError::InvalidAccountData);
    }

    // Return rent to depositor from escrow account
    let escrow_lamports = escrow_account.lamports();
    **escrow_account.try_borrow_mut_lamports()? = 0;
    **depositor.try_borrow_mut_lamports()? = depositor
        .lamports()
        .checked_add(escrow_lamports)
        .ok_or(ProgramError::ArithmeticOverflow)?;

    // Close vault too
    let vault_lamports = vault.lamports();
    if vault_lamports > 0 {
        **vault.try_borrow_mut_lamports()? = 0;
        **depositor.try_borrow_mut_lamports()? = depositor
            .lamports()
            .checked_add(vault_lamports)
            .ok_or(ProgramError::ArithmeticOverflow)?;
    }

    escrow.status = EscrowStatus::Cancelled;
    escrow.serialize(&mut &mut escrow_account.data.borrow_mut()[..])?;

    msg!("Escrow cancelled, rent returned to depositor");
    Ok(())
}
