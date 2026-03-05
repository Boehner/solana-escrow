use solana_program::program_error::ProgramError;

/// Custom error codes for the escrow program.
#[derive(Debug)]
pub enum EscrowError {
    InvalidState,
    Unauthorized,
    AmountMismatch,
}

impl From<EscrowError> for ProgramError {
    fn from(e: EscrowError) -> Self {
        ProgramError::Custom(e as u32)
    }
}
