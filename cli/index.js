#!/usr/bin/env node
/**
 * solana-escrow CLI — Interact with the on-chain escrow program.
 *
 * Usage:
 *   node cli/index.js init <recipient> <amount_sol> [description]
 *   node cli/index.js fund <escrow_address>
 *   node cli/index.js release <escrow_address>
 *   node cli/index.js dispute <escrow_address>
 *   node cli/index.js resolve <escrow_address> <recipient|depositor>
 *   node cli/index.js cancel <escrow_address>
 *   node cli/index.js status <escrow_address>
 */

const {
  Connection, Keypair, PublicKey, SystemProgram,
  Transaction, TransactionInstruction, LAMPORTS_PER_SOL,
  sendAndConfirmTransaction,
} = require("@solana/web3.js");
const borsh = require("borsh");
const fs = require("fs");
const path = require("path");

// --- Config ---
const RPC_URL = process.env.RPC_URL || "https://api.devnet.solana.com";
const PROGRAM_ID_FILE = path.join(__dirname, "..", "program-id.json");
const KEYPAIR_FILE = process.env.KEYPAIR || "C:\\openclaw\\solana\\id.json";
const FEE_WALLET = new PublicKey("FPRmCVAhz9eeLLAfKaangWaCuBgVmGAyYv99Yc616XdX");
const FEE_BPS = 200; // 2%

function loadProgramId() {
  if (!fs.existsSync(PROGRAM_ID_FILE)) {
    console.error("Error: program-id.json not found. Deploy the program first.");
    process.exit(1);
  }
  return new PublicKey(JSON.parse(fs.readFileSync(PROGRAM_ID_FILE, "utf8")).programId);
}

function loadKeypair() {
  const raw = JSON.parse(fs.readFileSync(KEYPAIR_FILE, "utf8"));
  return Keypair.fromSecretKey(Uint8Array.from(raw));
}

// --- Borsh schemas ---

// Instruction enum encoding (Borsh enum = 1 byte tag + fields)
function encodeInitialize(amount, description) {
  const buf = Buffer.alloc(1 + 8 + 32);
  buf.writeUInt8(0, 0); // tag: Initialize
  buf.writeBigUInt64LE(BigInt(amount), 1);
  const descBytes = Buffer.alloc(32);
  Buffer.from(description.slice(0, 32)).copy(descBytes);
  descBytes.copy(buf, 9);
  return buf;
}

function encodeFund() {
  return Buffer.from([1]); // tag: Fund
}

function encodeRelease() {
  return Buffer.from([2]); // tag: Release
}

function encodeDispute() {
  return Buffer.from([3]); // tag: Dispute
}

function encodeResolve(releaseToRecipient) {
  const buf = Buffer.alloc(2);
  buf.writeUInt8(4, 0); // tag: Resolve
  buf.writeUInt8(releaseToRecipient ? 1 : 0, 1);
  return buf;
}

function encodeCancel() {
  return Buffer.from([5]); // tag: Cancel
}

// --- PDA derivation ---
function findEscrowPDA(programId, depositor, recipient) {
  return PublicKey.findProgramAddressSync(
    [Buffer.from("escrow"), depositor.toBuffer(), recipient.toBuffer()],
    programId
  );
}

function findVaultPDA(programId, escrowPda) {
  return PublicKey.findProgramAddressSync(
    [Buffer.from("vault"), escrowPda.toBuffer()],
    programId
  );
}

// --- Escrow state decoding ---
function decodeEscrowState(data) {
  const statusNames = ["Initialized", "Funded", "Released", "Cancelled", "Disputed"];
  let offset = 0;

  const depositor = new PublicKey(data.slice(offset, offset + 32)); offset += 32;
  const recipient = new PublicKey(data.slice(offset, offset + 32)); offset += 32;
  const amount = data.readBigUInt64LE(offset); offset += 8;
  const statusByte = data.readUInt8(offset); offset += 1;
  const description = data.slice(offset, offset + 32); offset += 32;
  const escrowBump = data.readUInt8(offset); offset += 1;
  const vaultBump = data.readUInt8(offset); offset += 1;

  return {
    depositor: depositor.toBase58(),
    recipient: recipient.toBase58(),
    amount: Number(amount),
    amountSol: Number(amount) / LAMPORTS_PER_SOL,
    status: statusNames[statusByte] || `Unknown(${statusByte})`,
    description: Buffer.from(description).toString("utf8").replace(/\0/g, ""),
    escrowBump,
    vaultBump,
  };
}

// --- Commands ---

async function cmdInit(recipientStr, amountSol, description = "") {
  const conn = new Connection(RPC_URL, "confirmed");
  const programId = loadProgramId();
  const payer = loadKeypair();
  const recipient = new PublicKey(recipientStr);
  const lamports = Math.round(amountSol * LAMPORTS_PER_SOL);

  const [escrowPda] = findEscrowPDA(programId, payer.publicKey, recipient);
  const [vaultPda] = findVaultPDA(programId, escrowPda);

  console.log(`Initializing escrow:`);
  console.log(`  Depositor:  ${payer.publicKey.toBase58()}`);
  console.log(`  Recipient:  ${recipient.toBase58()}`);
  console.log(`  Amount:     ${amountSol} SOL (${lamports} lamports)`);
  console.log(`  Escrow PDA: ${escrowPda.toBase58()}`);
  console.log(`  Vault PDA:  ${vaultPda.toBase58()}`);

  const ix = new TransactionInstruction({
    programId,
    keys: [
      { pubkey: payer.publicKey, isSigner: true, isWritable: true },
      { pubkey: recipient, isSigner: false, isWritable: false },
      { pubkey: escrowPda, isSigner: false, isWritable: true },
      { pubkey: vaultPda, isSigner: false, isWritable: true },
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
    ],
    data: encodeInitialize(lamports, description),
  });

  const tx = new Transaction().add(ix);
  const sig = await sendAndConfirmTransaction(conn, tx, [payer]);
  console.log(`\n  TX: https://explorer.solana.com/tx/${sig}?cluster=devnet`);
  console.log(`  Escrow created. Now run: node cli/index.js fund ${escrowPda.toBase58()}`);
}

async function cmdFund(escrowStr) {
  const conn = new Connection(RPC_URL, "confirmed");
  const programId = loadProgramId();
  const payer = loadKeypair();
  const escrowPda = new PublicKey(escrowStr);
  const [vaultPda] = findVaultPDA(programId, escrowPda);

  const ix = new TransactionInstruction({
    programId,
    keys: [
      { pubkey: payer.publicKey, isSigner: true, isWritable: true },
      { pubkey: escrowPda, isSigner: false, isWritable: true },
      { pubkey: vaultPda, isSigner: false, isWritable: true },
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
    ],
    data: encodeFund(),
  });

  const tx = new Transaction().add(ix);
  const sig = await sendAndConfirmTransaction(conn, tx, [payer]);
  console.log(`Escrow funded!`);
  console.log(`  TX: https://explorer.solana.com/tx/${sig}?cluster=devnet`);
}

async function cmdRelease(escrowStr) {
  const conn = new Connection(RPC_URL, "confirmed");
  const programId = loadProgramId();
  const payer = loadKeypair();
  const escrowPda = new PublicKey(escrowStr);
  const [vaultPda] = findVaultPDA(programId, escrowPda);

  // Read escrow to get recipient
  const acct = await conn.getAccountInfo(escrowPda);
  if (!acct) { console.error("Escrow not found"); return; }
  const state = decodeEscrowState(acct.data);
  const recipient = new PublicKey(state.recipient);

  const fee = Math.floor(state.amount * FEE_BPS / 10000);
  const payout = state.amount - fee;

  const ix = new TransactionInstruction({
    programId,
    keys: [
      { pubkey: payer.publicKey, isSigner: true, isWritable: true },
      { pubkey: recipient, isSigner: false, isWritable: true },
      { pubkey: escrowPda, isSigner: false, isWritable: true },
      { pubkey: vaultPda, isSigner: false, isWritable: true },
      { pubkey: FEE_WALLET, isSigner: false, isWritable: true },
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
    ],
    data: encodeRelease(),
  });

  const tx = new Transaction().add(ix);
  const sig = await sendAndConfirmTransaction(conn, tx, [payer]);
  console.log(`Escrow released to ${state.recipient}!`);
  console.log(`  Payout: ${payout / LAMPORTS_PER_SOL} SOL | Fee: ${fee / LAMPORTS_PER_SOL} SOL (2%)`);
  console.log(`  TX: https://explorer.solana.com/tx/${sig}?cluster=devnet`);
}

async function cmdDispute(escrowStr) {
  const conn = new Connection(RPC_URL, "confirmed");
  const programId = loadProgramId();
  const payer = loadKeypair();
  const escrowPda = new PublicKey(escrowStr);

  const ix = new TransactionInstruction({
    programId,
    keys: [
      { pubkey: payer.publicKey, isSigner: true, isWritable: true },
      { pubkey: escrowPda, isSigner: false, isWritable: true },
    ],
    data: encodeDispute(),
  });

  const tx = new Transaction().add(ix);
  const sig = await sendAndConfirmTransaction(conn, tx, [payer]);
  console.log(`Dispute raised!`);
  console.log(`  TX: https://explorer.solana.com/tx/${sig}?cluster=devnet`);
}

async function cmdResolve(escrowStr, target) {
  const conn = new Connection(RPC_URL, "confirmed");
  const programId = loadProgramId();
  const payer = loadKeypair();
  const escrowPda = new PublicKey(escrowStr);
  const [vaultPda] = findVaultPDA(programId, escrowPda);
  const releaseToRecipient = target === "recipient";

  const acct = await conn.getAccountInfo(escrowPda);
  if (!acct) { console.error("Escrow not found"); return; }
  const state = decodeEscrowState(acct.data);

  const fee = Math.floor(state.amount * FEE_BPS / 10000);
  const payout = state.amount - fee;

  const ix = new TransactionInstruction({
    programId,
    keys: [
      { pubkey: payer.publicKey, isSigner: true, isWritable: true },
      { pubkey: new PublicKey(state.depositor), isSigner: false, isWritable: true },
      { pubkey: new PublicKey(state.recipient), isSigner: false, isWritable: true },
      { pubkey: escrowPda, isSigner: false, isWritable: true },
      { pubkey: vaultPda, isSigner: false, isWritable: true },
      { pubkey: FEE_WALLET, isSigner: false, isWritable: true },
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
    ],
    data: encodeResolve(releaseToRecipient),
  });

  const tx = new Transaction().add(ix);
  const sig = await sendAndConfirmTransaction(conn, tx, [payer]);
  console.log(`Dispute resolved! Funds sent to ${target}.`);
  console.log(`  Payout: ${payout / LAMPORTS_PER_SOL} SOL | Fee: ${fee / LAMPORTS_PER_SOL} SOL (2%)`);
  console.log(`  TX: https://explorer.solana.com/tx/${sig}?cluster=devnet`);
}

async function cmdCancel(escrowStr) {
  const conn = new Connection(RPC_URL, "confirmed");
  const programId = loadProgramId();
  const payer = loadKeypair();
  const escrowPda = new PublicKey(escrowStr);
  const [vaultPda] = findVaultPDA(programId, escrowPda);

  const ix = new TransactionInstruction({
    programId,
    keys: [
      { pubkey: payer.publicKey, isSigner: true, isWritable: true },
      { pubkey: escrowPda, isSigner: false, isWritable: true },
      { pubkey: vaultPda, isSigner: false, isWritable: true },
    ],
    data: encodeCancel(),
  });

  const tx = new Transaction().add(ix);
  const sig = await sendAndConfirmTransaction(conn, tx, [payer]);
  console.log(`Escrow cancelled. Rent returned.`);
  console.log(`  TX: https://explorer.solana.com/tx/${sig}?cluster=devnet`);
}

async function cmdStatus(escrowStr) {
  const conn = new Connection(RPC_URL, "confirmed");
  const escrowPda = new PublicKey(escrowStr);
  const programId = loadProgramId();
  const [vaultPda] = findVaultPDA(programId, escrowPda);

  const acct = await conn.getAccountInfo(escrowPda);
  if (!acct) { console.error("Escrow account not found"); return; }

  const state = decodeEscrowState(acct.data);
  const vaultAcct = await conn.getAccountInfo(vaultPda);
  const vaultBalance = vaultAcct ? vaultAcct.lamports : 0;

  console.log(`\n=== Escrow Status ===`);
  console.log(`  Address:     ${escrowStr}`);
  console.log(`  Depositor:   ${state.depositor}`);
  console.log(`  Recipient:   ${state.recipient}`);
  console.log(`  Amount:      ${state.amountSol} SOL (${state.amount} lamports)`);
  console.log(`  Status:      ${state.status}`);
  console.log(`  Description: ${state.description || "(none)"}`);
  console.log(`  Vault:       ${vaultPda.toBase58()}`);
  console.log(`  Vault bal:   ${vaultBalance / LAMPORTS_PER_SOL} SOL`);
  console.log(`  Explorer:    https://explorer.solana.com/address/${escrowStr}?cluster=devnet`);
}

// --- Main ---
async function main() {
  const [,, cmd, ...args] = process.argv;

  if (!cmd) {
    console.log("Usage:");
    console.log("  node cli/index.js init <recipient> <amount_sol> [description]");
    console.log("  node cli/index.js fund <escrow_address>");
    console.log("  node cli/index.js release <escrow_address>");
    console.log("  node cli/index.js dispute <escrow_address>");
    console.log("  node cli/index.js resolve <escrow_address> <recipient|depositor>");
    console.log("  node cli/index.js cancel <escrow_address>");
    console.log("  node cli/index.js status <escrow_address>");
    return;
  }

  try {
    switch (cmd) {
      case "init":    await cmdInit(args[0], parseFloat(args[1]), args[2] || ""); break;
      case "fund":    await cmdFund(args[0]); break;
      case "release": await cmdRelease(args[0]); break;
      case "dispute": await cmdDispute(args[0]); break;
      case "resolve": await cmdResolve(args[0], args[1]); break;
      case "cancel":  await cmdCancel(args[0]); break;
      case "status":  await cmdStatus(args[0]); break;
      default:        console.error(`Unknown command: ${cmd}`);
    }
  } catch (e) {
    console.error(`Error: ${e.message}`);
    if (e.logs) console.error("Program logs:", e.logs);
  }
}

main();
