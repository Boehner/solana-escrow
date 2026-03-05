# solana-escrow

A trustless escrow service rebuilt from a traditional Web2 backend pattern into an on-chain Solana program. Funds are held in a Program Derived Address (PDA) and released by deterministic instruction logic — no trusted intermediary required.

**Built for the [Superteam Poland "Rebuild Backend Systems as On-Chain Rust Programs" bounty](https://superteam.fun/earn/listings/bounties/rebuild-production-backend-systems-as-on-chain-rust-programs).**

## Architecture: Web2 vs Solana

### The Web2 Escrow

A typical Web2 escrow service consists of:

```
┌─────────────┐     ┌──────────────┐     ┌──────────────┐
│  REST API   │────▶│ Escrow Logic │────▶│  Database     │
│  (Express)  │     │  (Node.js)   │     │  (PostgreSQL) │
└─────────────┘     └──────────────┘     └──────────────┘
       │                    │
       │              ┌─────▼──────┐
       │              │ Payment    │
       └─────────────▶│ Provider   │  (Stripe, PayPal)
                      └────────────┘
```

- **Trust model**: Users trust the escrow company to hold funds honestly
- **State**: Rows in a SQL database (`escrows` table with status column)
- **Authorization**: JWT tokens, session cookies, OAuth
- **Payments**: Integrated with Stripe/PayPal — another trusted third party
- **Disputes**: Resolved by human support staff
- **Uptime**: Depends on company's infrastructure

### The Solana Escrow

```
┌──────────────┐     ┌──────────────────────────────┐
│  CLI Client  │────▶│  Solana Program (on-chain)    │
│  (Node.js)   │     │  ┌────────────┐  ┌─────────┐ │
└──────────────┘     │  │ Instruction│  │ Escrow  │ │
                     │  │ Processor  │──▶│ State   │ │
                     │  └────────────┘  │ (PDA)   │ │
                     │                  └─────────┘ │
                     │  ┌────────────┐              │
                     │  │ Vault PDA  │ holds SOL    │
                     │  └────────────┘              │
                     └──────────────────────────────┘
                              │
                     ┌────────▼────────┐
                     │  Solana Ledger  │  (the "database")
                     └─────────────────┘
```

- **Trust model**: Trustless — program logic is deterministic, verifiable, immutable
- **State**: PDA account data, serialized with Borsh (replaces SQL database)
- **Authorization**: Ed25519 cryptographic signatures (replaces passwords/tokens)
- **Payments**: Native SOL transfers via the System Program — no third party
- **Disputes**: Resolved by on-chain logic (arbiter keypair)
- **Uptime**: Solana network uptime (~99.5%) — no single point of failure

### Mapping: Web2 Concepts → Solana Equivalents

| Web2 Concept | Solana Equivalent | Key Difference |
|---|---|---|
| Database row | PDA account data | Data lives on-chain, globally replicated |
| Primary key / row ID | PDA address (derived from seeds) | Deterministic — anyone can compute it |
| `INSERT INTO escrows` | `create_account` + serialize state | Account creation costs rent (refundable) |
| `UPDATE escrows SET status=` | Deserialize → modify → reserialize | Program must own the account to write |
| REST API endpoint | Instruction handler | Client builds transaction, validators execute |
| User authentication | Transaction signer | No passwords — cryptographic proof of ownership |
| Bank transfer | `system_instruction::transfer` | Atomic, instant settlement, no intermediary |
| Escrow company holds funds | Vault PDA holds lamports | No human can access vault — only program logic |
| Support ticket for disputes | `Dispute` + `Resolve` instructions | Transparent, auditable, deterministic |
| Server logs | Transaction history on Solana Explorer | Permanent, public, tamper-proof |

### Tradeoffs and Constraints

| Aspect | Web2 Advantage | Solana Advantage |
|---|---|---|
| **Flexibility** | Can handle any business logic, free-form data | Constrained by on-chain compute limits (200K CU) |
| **Cost** | $0 per transaction (absorbed by business) | ~$0.00025 per transaction + rent for state |
| **Reversibility** | Admin can always override | Immutable once deployed — feature, not bug |
| **Privacy** | Data is private by default | All state is public on-chain |
| **Speed** | Millisecond response | ~400ms block time, confirmation in seconds |
| **Account model** | One big database, flexible queries | Each escrow = separate account, no SQL queries |
| **State size** | Unlimited (just add more DB storage) | Fixed at account creation (106 bytes for our Escrow) |
| **Dispute resolution** | Flexible human judgment | Deterministic binary (release or refund) |

### Design Decisions

1. **Native SDK over Anchor**: Used `solana-program` directly to demonstrate the raw account model without framework abstraction. This shows exactly how Solana state management works — PDAs, account ownership, serialization — without hiding it behind macros.

2. **Fixed-size description field** (32 bytes): In Web2, you'd use a `TEXT` column. On Solana, account size is fixed at creation and costs rent proportional to size. A 32-byte field is a pragmatic tradeoff — enough for an order hash or short reference, without inflating rent costs.

3. **Depositor-as-arbiter simplification**: A production system would have a third-party arbiter keypair. We simplified to keep the account model minimal while demonstrating the dispute flow.

4. **Separate vault PDA**: Funds are held in a dedicated vault account rather than the escrow state account. This separates data from value — a pattern that maps to Web2's separation of the `escrows` table from the actual payment processor balance.

## Program Instructions

| Instruction | Web2 Equivalent | Description |
|---|---|---|
| `Initialize` | `POST /escrow` | Create escrow agreement between depositor and recipient |
| `Fund` | `POST /escrow/:id/fund` | Depositor sends agreed SOL amount to vault |
| `Release` | `POST /escrow/:id/release` | Depositor authorizes release to recipient |
| `Dispute` | `POST /escrow/:id/dispute` | Either party raises a dispute |
| `Resolve` | `POST /escrow/:id/resolve` | Arbiter resolves dispute (release or refund) |
| `Cancel` | `DELETE /escrow/:id` | Cancel unfunded escrow, reclaim rent |

## State Machine

```
  Initialize          Fund            Release
     │                  │                │
     ▼                  ▼                ▼
 ┌────────────┐   ┌──────────┐   ┌──────────────┐
 │Initialized │──▶│  Funded  │──▶│   Released   │
 └────────────┘   └──────────┘   └──────────────┘
      │                │
      │ Cancel         │ Dispute
      ▼                ▼
 ┌──────────┐   ┌──────────────┐   Resolve
 │Cancelled │   │  Disputed    │──────────┐
 └──────────┘   └──────────────┘          │
                                          ▼
                                  Released or Cancelled
```

## Build & Deploy

### Prerequisites

- Rust + Solana CLI (`solana-cli 3.x`)
- Node.js 18+

### Build the program

```bash
cd program
cargo-build-sbf
```

Output: `target/deploy/solana_escrow.so`

### Deploy to devnet

```bash
solana config set --url devnet
solana airdrop 2
solana program deploy target/deploy/solana_escrow.so
```

Save the program ID to `program-id.json`:
```json
{ "programId": "<YOUR_PROGRAM_ID>" }
```

### CLI Usage

```bash
cd cli && npm install

# Create an escrow: deposit 0.5 SOL for a recipient
node index.js init <RECIPIENT_PUBKEY> 0.5 "order-12345"

# Fund the escrow
node index.js fund <ESCROW_PDA>

# Check status
node index.js status <ESCROW_PDA>

# Release to recipient (happy path)
node index.js release <ESCROW_PDA>

# Or: raise a dispute
node index.js dispute <ESCROW_PDA>

# Resolve dispute
node index.js resolve <ESCROW_PDA> recipient   # release to recipient
node index.js resolve <ESCROW_PDA> depositor    # refund to depositor

# Cancel unfunded escrow
node index.js cancel <ESCROW_PDA>
```

## Devnet Transactions

- **Program deploy**: [F1kc16E...](https://explorer.solana.com/tx/F1kc16E6Bke9KYBWSk3LVAtZAJEC6Etmuc7nPbKijBhhHMDA8RandEVeN5HVPikHK1EwEhk9PaxcYtcL2Eki6Fj?cluster=devnet)
- **Program ID**: [`FGnitAfLPArVcNfR3pVKvp4ucpZNjoRsj9YyXw5q519o`](https://explorer.solana.com/address/FGnitAfLPArVcNfR3pVKvp4ucpZNjoRsj9YyXw5q519o?cluster=devnet)
- **Initialize escrow** (0.05 SOL): [c7h24uL...](https://explorer.solana.com/tx/c7h24uLAEbcoEPcQfhCrDgqECDrsi2sPLG5oew2mj4e3S7HhcPXNXZrVJFpkaWVD2UxuaXGfwnwXEKz9jtPWbzA?cluster=devnet)
- **Fund escrow**: [4T4XwP4...](https://explorer.solana.com/tx/4T4XwP4EUrxtVQdLYeEa6ECjMrMWAALq5r8PwVrDMuk3khFsiHhUUTw1E3krk4hd5EsxdERc9zyCb6NQHCHwvjax?cluster=devnet)
- **Release to recipient**: [2jVnMxa...](https://explorer.solana.com/tx/2jVnMxaht99PRsLWeCCk9emnZNA97eHyG8rHDXsfdCuYAo55TEL9NzLTUa5zyYNm8WhcvdxKLPeF4FCBPNtn4FWM?cluster=devnet)

## Project Structure

```
solana-escrow/
├── program/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs          # Entry point + instruction handlers
│       ├── state.rs         # Escrow state struct + status enum
│       ├── instruction.rs   # Instruction enum (Borsh-serialized)
│       └── error.rs         # Custom error types
├── cli/
│   ├── package.json
│   └── index.js             # Node.js CLI client
├── program-id.json          # Deployed program address
└── README.md
```

## License

MIT
