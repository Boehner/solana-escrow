use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::pubkey::Pubkey;

/// Escrow lifecycle states — mirrors a Web2 escrow's state machine:
///
///   Web2:   CREATED → FUNDED → RELEASED / REFUNDED / DISPUTED → RESOLVED
///   Solana: Initialized → Funded → Released / Cancelled / Disputed → Resolved
///
/// Key difference: In Web2, the escrow service enforces transitions.
/// On Solana, the program's instruction logic enforces them — no human operator.
#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, Copy, PartialEq)]
pub enum EscrowStatus {
    Initialized,  // Created but not yet funded
    Funded,        // Depositor has sent funds to vault
    Released,      // Funds released to recipient
    Cancelled,     // Escrow cancelled, funds returned to depositor
    Disputed,      // One party raised a dispute
}

/// On-chain escrow state.
///
/// Web2 equivalent: a row in an `escrows` database table with columns for
/// depositor_id, recipient_id, amount, status, description.
///
/// Solana equivalent: a PDA account whose data is this struct, serialized
/// with Borsh. The "database" is the Solana ledger; the "row ID" is the PDA
/// derived from [b"escrow", depositor, recipient].
#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct Escrow {
    /// The party depositing funds (Web2: "buyer" or "sender").
    pub depositor: Pubkey,

    /// The party receiving funds on release (Web2: "seller" or "recipient").
    pub recipient: Pubkey,

    /// Agreed escrow amount in lamports.
    pub amount: u64,

    /// Current lifecycle state.
    pub status: EscrowStatus,

    /// Short description/reference (32 bytes, e.g. order ID hash).
    /// In Web2, this would be a text field; here we use fixed bytes for
    /// deterministic account sizing.
    pub description: [u8; 32],

    /// PDA bump seeds for re-derivation.
    pub escrow_bump: u8,
    pub vault_bump: u8,
}
