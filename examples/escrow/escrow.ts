/**
 * Reference client for the stabl_escrow contract.
 *
 * Shows how an off-chain client:
 *   - computes `terms_hash` (format v1) from a quote,
 *   - reads an escrow and checks it against the quote it expects,
 *   - claims it as the recipient (`release_escrow`),
 *   - refunds it after expiry (`refund_escrow`, anyone may call),
 *   - locks funds as the sender (`lock_for_offramp`).
 *
 * Usage (from this directory, after `npm install`):
 *   npx tsx escrow.ts hash   --quote-id <uuid> --beneficiary-id <uuid> --token <C...> --amount <i128> --expiry <unix>
 *   npx tsx escrow.ts check  --sender <G...|C...> --tx-id <uuid> [--quote-id <uuid> --beneficiary-id <uuid>]
 *   RECIPIENT_SECRET=S... npx tsx escrow.ts claim  --sender <G...> --tx-id <uuid>
 *   SOURCE_SECRET=S...    npx tsx escrow.ts refund --sender <G...> --tx-id <uuid>
 *   SENDER_SECRET=S...    npx tsx escrow.ts lock   --recipient <G...> --tx-id <uuid> --token <C...> \
 *                           --amount <i128> --expiry <unix> --quote-id <uuid> --beneficiary-id <uuid>
 *
 * Network and contract ID come from ../../deployments/<network>.json
 * (`--network testnet` by default). Never hard-code contract IDs.
 */
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { parseArgs } from "node:util";
import {
  Account,
  Address,
  BASE_FEE,
  Contract,
  Keypair,
  TransactionBuilder,
  nativeToScVal,
  rpc,
  scValToNative,
  xdr,
} from "@stellar/stellar-sdk";

// ---------------------------------------------------------------------------
// terms_hash v1
//
// sha256( 0x01                                   version
//      || quote_id        36 bytes  lowercase UUID text
//      || beneficiary_id  36 bytes  lowercase UUID text
//      || token           56 bytes  C... strkey text
//      || amount          16 bytes  i128 big-endian
//      || expiry           8 bytes  u64 big-endian )
//
// Every field is fixed-length, so plain concatenation is unambiguous. The
// contract never computes this; it stores the 32 bytes the sender signed.
// ---------------------------------------------------------------------------

const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

export interface Terms {
  quoteId: string;
  beneficiaryId: string;
  token: string;
  amount: bigint;
  expiry: bigint;
}

export function termsHashV1(t: Terms): Buffer {
  for (const [name, id] of [["quoteId", t.quoteId], ["beneficiaryId", t.beneficiaryId]]) {
    if (!UUID_RE.test(id)) throw new Error(`${name} must be a lowercase hyphenated UUID: ${id}`);
  }
  if (!/^C[A-Z2-7]{55}$/.test(t.token)) throw new Error(`token must be a C... contract strkey: ${t.token}`);

  const amount = Buffer.alloc(16);
  amount.writeBigInt64BE(t.amount >> 64n, 0); // high 64 bits (signed)
  amount.writeBigUInt64BE(t.amount & 0xffff_ffff_ffff_ffffn, 8); // low 64 bits
  const expiry = Buffer.alloc(8);
  expiry.writeBigUInt64BE(t.expiry);

  return createHash("sha256")
    .update(Buffer.from([1]))
    .update(Buffer.from(t.quoteId, "ascii"))
    .update(Buffer.from(t.beneficiaryId, "ascii"))
    .update(Buffer.from(t.token, "ascii"))
    .update(amount)
    .update(expiry)
    .digest();
}

// ---------------------------------------------------------------------------
// Contract access
// ---------------------------------------------------------------------------

const STATES: Record<number, string> = { 1: "Locked", 2: "Claimed", 3: "Refunded" };

export interface EscrowEntry {
  recipient: string;
  state: string;
  params: { token: string; amount: bigint; expiry: bigint; terms_hash: Buffer };
}

function loadNetwork(network: string) {
  const path = new URL(`../../deployments/${network}.json`, import.meta.url);
  const manifest = JSON.parse(readFileSync(path, "utf8"));
  const id = manifest.contracts?.stabl_escrow?.id;
  if (!id) throw new Error(`stabl_escrow not deployed on ${network}`);
  return {
    server: new rpc.Server(manifest.rpc_url),
    passphrase: manifest.network_passphrase as string,
    contract: new Contract(id),
  };
}

const addr = (a: string) => new Address(a).toScVal();
const str = (s: string) => nativeToScVal(s, { type: "string" });

/** Read-only call: simulate only, nothing is submitted or signed. */
async function getEscrowEntry(network: string, sender: string, txId: string): Promise<EscrowEntry> {
  const { server, passphrase, contract } = loadNetwork(network);
  // Simulation needs a source account but never submits, so any key will do.
  const source = new Account(Keypair.random().publicKey(), "0");
  const tx = new TransactionBuilder(source, { fee: BASE_FEE, networkPassphrase: passphrase })
    .addOperation(contract.call("get_escrow_entry", addr(sender), str(txId)))
    .setTimeout(30)
    .build();
  const sim = await server.simulateTransaction(tx);
  if (rpc.Api.isSimulationError(sim)) {
    // Contract errors surface here, e.g. "Error(Contract, #1000)" = NotFound.
    throw new Error(`get_escrow_entry failed: ${sim.error}`);
  }
  const raw = scValToNative(sim.result!.retval);
  return {
    recipient: raw.recipient,
    // EscrowState is a u32-backed enum on the contract: 1 Locked, 2 Claimed, 3 Refunded.
    state: STATES[raw.state] ?? `Unknown(${raw.state})`,
    params: { ...raw.params, terms_hash: Buffer.from(raw.params.terms_hash) },
  };
}

/** Build, simulate (which fills in auth and resources), sign and submit. */
async function invoke(network: string, signer: Keypair, method: string, args: xdr.ScVal[]) {
  const { server, passphrase, contract } = loadNetwork(network);
  const account = await server.getAccount(signer.publicKey());
  const built = new TransactionBuilder(account, { fee: BASE_FEE, networkPassphrase: passphrase })
    .addOperation(contract.call(method, ...args))
    .setTimeout(60)
    .build();
  const tx = await server.prepareTransaction(built); // throws on simulation error
  tx.sign(signer);
  const sent = await server.sendTransaction(tx);
  if (sent.status === "ERROR") throw new Error(`${method} rejected: ${JSON.stringify(sent.errorResult)}`);

  for (let i = 0; i < 30; i++) {
    const res = await server.getTransaction(sent.hash);
    if (res.status === rpc.Api.GetTransactionStatus.SUCCESS) return { hash: sent.hash };
    if (res.status === rpc.Api.GetTransactionStatus.FAILED) throw new Error(`${method} failed: ${sent.hash}`);
    await new Promise((r) => setTimeout(r, 1000));
  }
  throw new Error(`${method} not confirmed after 30s: ${sent.hash}`);
}

/** The escrow params struct: an ScMap whose symbol keys must be sorted. */
function paramsScVal(token: string, amount: bigint, expiry: bigint, termsHash: Buffer): xdr.ScVal {
  const entry = (key: string, val: xdr.ScVal) =>
    new xdr.ScMapEntry({ key: xdr.ScVal.scvSymbol(key), val });
  return xdr.ScVal.scvMap([
    entry("amount", nativeToScVal(amount, { type: "i128" })),
    entry("expiry", nativeToScVal(expiry, { type: "u64" })),
    entry("terms_hash", xdr.ScVal.scvBytes(termsHash)),
    entry("token", addr(token)),
  ]);
}

function secret(name: string): Keypair {
  const s = process.env[name];
  if (!s) throw new Error(`set ${name}=S... in the environment`);
  return Keypair.fromSecret(s);
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

async function main() {
  const [command, ...rest] = process.argv.slice(2);
  const { values: o } = parseArgs({
    args: rest,
    options: {
      network: { type: "string", default: "testnet" },
      sender: { type: "string" },
      recipient: { type: "string" },
      "tx-id": { type: "string" },
      "quote-id": { type: "string" },
      "beneficiary-id": { type: "string" },
      token: { type: "string" },
      amount: { type: "string" },
      expiry: { type: "string" },
    },
  });
  const need = (k: keyof typeof o) => {
    const v = o[k];
    if (typeof v !== "string") throw new Error(`--${k} is required`);
    return v;
  };

  switch (command) {
    case "hash": {
      const h = termsHashV1({
        quoteId: need("quote-id"),
        beneficiaryId: need("beneficiary-id"),
        token: need("token"),
        amount: BigInt(need("amount")),
        expiry: BigInt(need("expiry")),
      });
      console.log(h.toString("hex"));
      break;
    }

    case "check": {
      const entry = await getEscrowEntry(o.network!, need("sender"), need("tx-id"));
      const now = BigInt(Math.floor(Date.now() / 1000));
      console.log({
        recipient: entry.recipient,
        state: entry.state,
        token: entry.params.token,
        amount: entry.params.amount.toString(),
        expiry: `${entry.params.expiry} (${new Date(Number(entry.params.expiry) * 1000).toISOString()})`,
        claimable: entry.state === "Locked" && now <= entry.params.expiry,
        terms_hash: entry.params.terms_hash.toString("hex"),
      });
      if (o["quote-id"] && o["beneficiary-id"]) {
        // Recompute from the quote you hold and compare with what the sender signed.
        const expected = termsHashV1({
          quoteId: o["quote-id"],
          beneficiaryId: o["beneficiary-id"],
          token: entry.params.token,
          amount: entry.params.amount,
          expiry: entry.params.expiry,
        });
        const ok = expected.equals(entry.params.terms_hash);
        console.log(ok ? "terms_hash matches the quote" : `terms_hash MISMATCH, expected ${expected.toString("hex")}`);
        if (!ok) process.exitCode = 1;
      }
      break;
    }

    case "claim": {
      const res = await invoke(o.network!, secret("RECIPIENT_SECRET"), "release_escrow", [
        addr(need("sender")),
        str(need("tx-id")),
      ]);
      console.log("claimed:", res.hash);
      break;
    }

    case "refund": {
      const res = await invoke(o.network!, secret("SOURCE_SECRET"), "refund_escrow", [
        addr(need("sender")),
        str(need("tx-id")),
      ]);
      console.log("refunded:", res.hash);
      break;
    }

    case "lock": {
      const sender = secret("SENDER_SECRET");
      const token = need("token");
      const amount = BigInt(need("amount"));
      const expiry = BigInt(need("expiry"));
      const termsHash = termsHashV1({
        quoteId: need("quote-id"),
        beneficiaryId: need("beneficiary-id"),
        token,
        amount,
        expiry,
      });
      const res = await invoke(o.network!, sender, "lock_for_offramp", [
        addr(sender.publicKey()),
        addr(need("recipient")),
        str(need("tx-id")),
        paramsScVal(token, amount, expiry, termsHash),
      ]);
      console.log("locked:", res.hash, "terms_hash:", termsHash.toString("hex"));
      break;
    }

    default:
      console.error("commands: hash | check | claim | refund | lock (see header of escrow.ts)");
      process.exitCode = 2;
  }
}

main().catch((err) => {
  console.error(err instanceof Error ? err.message : err);
  process.exitCode = 1;
});
