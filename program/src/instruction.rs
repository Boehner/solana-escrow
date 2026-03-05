use borsh::{BorshDeserialize, BorshSerialize};

/// Escrow instructions — each maps to a Web2 API endpoint:
///
/// | Web2 REST Endpoint        | Solana Instruction | Notes                           |
/// |---------------------------|--------------------|---------------------------------|
/// | POST /escrow              | Initialize         | Create new escrow agreement     |
/// | POST /escrow/:id/fund     | Fund               | Depositor sends funds           |
/// | POST /escrow/:id/release  | Release            | Depositor authorizes payout     |
/// | POST /escrow/:id/dispute  | Dispute            | Either party raises dispute     |
/// | POST /escrow/:id/resolve  | Resolve            | Arbiter resolves dispute        |
/// | DELETE /escrow/:id        | Cancel             | Cancel unfunded escrow          |
#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub enum EscrowInstruction {
    /// Create a new escrow.
    /// Accounts: [depositor (signer), recipient, escrow_pda, vault_pda, system_program]
    Initialize {
        amount: u64,
        description: [u8; 32],
    },

    /// Fund the escrow with the agreed amount.
    /// Accounts: [depositor (signer), escrow_pda, vault_pda, system_program]
    Fund,

    /// Release funds to recipient. Depositor authorizes.
    /// Accounts: [depositor (signer), recipient, escrow_pda, vault_pda]
    Release,

    /// Raise a dispute. Either party can call this.
    /// Accounts: [disputer (signer), escrow_pda]
    Dispute,

    /// Resolve a dispute. Currently depositor acts as arbiter.
    /// Accounts: [arbiter (signer), depositor, recipient, escrow_pda, vault_pda]
    Resolve {
        release_to_recipient: bool,
    },

    /// Cancel an unfunded escrow and reclaim rent.
    /// Accounts: [depositor (signer), escrow_pda, vault_pda]
    Cancel,
}
