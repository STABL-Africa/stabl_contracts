// Browser harness: drive a testnet stabl_multi_signer account with a
// real passkey. Everything an off-chain client has to do is done here in the
// page, so this file is the reference implementation for server-side code.
//
// Flow for an authorised call:
//   1. build tx invoking the account contract, simulate
//   2. for each auth entry addressed to the account:
//        payload     = sha256(xdr(HashIdPreimage::SorobanAuthorization))
//        auth_digest = sha256(payload || xdr(ScVec<u32> context_rule_ids))
//        challenge   = auth_digest                    <- navigator.credentials.get
//        sig_data    = xdr(WebAuthnSigData{signature r||s low-s, authenticator_data, client_data})
//        signature   = AuthPayload{ signers: { External(verifier, key_data) => sig_data }, context_rule_ids }
//   3. rebuild tx with signed auth entries, simulate, assemble, sign with fee payer, send

import * as StellarSdk from "https://esm.sh/@stellar/stellar-sdk@14.1.1?bundle";
import { Buffer } from "https://esm.sh/buffer@6.0.3";

globalThis.Buffer ??= Buffer;

const { xdr, hash, Address, Keypair, TransactionBuilder, Contract, Operation, rpc, scValToNative } = StellarSdk;

const $ = (id) => document.getElementById(id);
const logEl = $("log");
const log = (...a) => {
  logEl.textContent += a.map((x) => (typeof x === "string" ? x : JSON.stringify(x, null, 2))).join(" ") + "\n";
  logEl.scrollTop = logEl.scrollHeight;
};

const hex = (u8) => Array.from(u8, (b) => b.toString(16).padStart(2, "0")).join("");
const unhex = (h) => Buffer.from(h.replace(/^0x/, "").replace(/\s+/g, ""), "hex");
const concat = (...parts) => Buffer.concat(parts.map((p) => Buffer.from(p)));

// secp256r1 order and half-order, for low-s normalisation.
const P256_N = BigInt("0xFFFFFFFF00000000FFFFFFFFFFFFFFFFBCE6FAADA7179E84F3B9CAC2FC632551");
const P256_HALF = P256_N >> 1n;

/** DER ECDSA signature -> 64-byte r||s with low s (Soroban rejects high s). */
function derToRawLowS(der) {
  const b = new Uint8Array(der);
  if (b[0] !== 0x30) throw new Error("bad DER: no SEQUENCE");
  let i = 2;
  if (b[i] !== 0x02) throw new Error("bad DER: no INTEGER r");
  const rLen = b[i + 1];
  const r = b.slice(i + 2, i + 2 + rLen);
  i += 2 + rLen;
  if (b[i] !== 0x02) throw new Error("bad DER: no INTEGER s");
  const sLen = b[i + 1];
  const s = b.slice(i + 2, i + 2 + sLen);
  const toBig = (u8) => BigInt("0x" + hex(u8));
  let sBig = toBig(s);
  if (sBig > P256_HALF) sBig = P256_N - sBig;
  const pad32 = (big) => unhex(big.toString(16).padStart(64, "0"));
  return concat(pad32(toBig(r)), pad32(sBig));
}

const sym = (s) => xdr.ScVal.scvSymbol(s);
const bytes = (b) => xdr.ScVal.scvBytes(Buffer.from(b));
const u32 = (n) => xdr.ScVal.scvU32(n);
const mapEntry = (key, val) => new xdr.ScMapEntry({ key, val });

/** WebAuthnSigData as ScVal (struct -> map with keys sorted by name). */
function webauthnSigData(authenticatorData, clientDataJSON, rawSig) {
  return xdr.ScVal.scvMap([
    mapEntry(sym("authenticator_data"), bytes(authenticatorData)),
    mapEntry(sym("client_data"), bytes(clientDataJSON)),
    mapEntry(sym("signature"), bytes(rawSig)),
  ]);
}

/** Signer::External(verifier, key_data) as ScVal (enum tuple variant -> vec). */
function externalSigner(verifier, keyData) {
  return xdr.ScVal.scvVec([sym("External"), new Address(verifier).toScVal(), bytes(keyData)]);
}

/** AuthPayload as ScVal. */
function authPayload(signerScVal, sigDataXdr, ruleIds) {
  return xdr.ScVal.scvMap([
    mapEntry(sym("context_rule_ids"), ruleIds),
    mapEntry(sym("signers"), xdr.ScVal.scvMap([mapEntry(signerScVal, bytes(sigDataXdr))])),
  ]);
}

async function passkeyAssert(challenge, credIdHex, rpId) {
  const cred = await navigator.credentials.get({
    publicKey: {
      challenge,
      rpId,
      allowCredentials: [{ type: "public-key", id: unhex(credIdHex) }],
      userVerification: "required",
      timeout: 120_000,
    },
  });
  const r = cred.response;
  return {
    authenticatorData: new Uint8Array(r.authenticatorData),
    clientDataJSON: new Uint8Array(r.clientDataJSON),
    rawSig: derToRawLowS(r.signature),
  };
}

async function pollTx(server, hashHex) {
  for (let i = 0; i < 30; i++) {
    const r = await server.getTransaction(hashHex);
    if (r.status !== "NOT_FOUND") return r;
    await new Promise((res) => setTimeout(res, 2000));
  }
  throw new Error("timed out waiting for transaction");
}

async function run() {
  logEl.textContent = "";
  const cfg = {
    rpc: $("rpc").value.trim(),
    passphrase: $("passphrase").value,
    account: $("account").value.trim(),
    verifier: $("verifier").value.trim(),
    keyData: $("keydata").value.trim(),
    credId: $("credid").value.trim(),
    rpId: $("rpid").value.trim(),
    secret: $("secret").value.trim(),
    op: $("op").value,
  };
  const server = new rpc.Server(cfg.rpc);
  const payer = Keypair.fromSecret(cfg.secret);
  const contract = new Contract(cfg.account);
  log("fee payer:", payer.publicKey());

  if (cfg.op === "rules_count") {
    const source = await server.getAccount(payer.publicKey());
    const tx = new TransactionBuilder(source, { fee: "100000", networkPassphrase: cfg.passphrase })
      .addOperation(contract.call("get_context_rules_count"))
      .setTimeout(60)
      .build();
    const sim = await server.simulateTransaction(tx);
    if (rpc.Api.isSimulationError(sim)) throw new Error(sim.error);
    log("get_context_rules_count =", scValToNative(sim.result.retval));
    return;
  }

  // ---- 1. build + simulate the authorised call -------------------------
  const newSigner = xdr.ScVal.scvVec([sym("Delegated"), new Address(payer.publicKey()).toScVal()]);
  const buildOp = (auth) =>
    Operation.invokeContractFunction({ contract: cfg.account, function: "add_signer", args: [u32(0), newSigner], auth });

  let source = await server.getAccount(payer.publicKey());
  const tx1 = new TransactionBuilder(source, { fee: "1000000", networkPassphrase: cfg.passphrase })
    .addOperation(buildOp([]))
    .setTimeout(300)
    .build();
  const sim1 = await server.simulateTransaction(tx1);
  if (rpc.Api.isSimulationError(sim1)) throw new Error("simulate #1: " + sim1.error);
  const entries = sim1.result.auth;
  log(`simulation returned ${entries.length} auth entr${entries.length === 1 ? "y" : "ies"}`);

  // ---- 2. sign each entry addressed to the smart account ----------------
  const latest = await server.getLatestLedger();
  const validUntil = latest.sequence + 60;
  const networkId = hash(Buffer.from(cfg.passphrase));
  const ruleIds = xdr.ScVal.scvVec([u32(0)]); // one per auth context; Default rule is 0
  const signerScVal = externalSigner(cfg.verifier, unhex(cfg.keyData));

  const signed = [];
  for (const entry of entries) {
    if (entry.credentials().switch() !== xdr.SorobanCredentialsType.sorobanCredentialsAddress()) {
      signed.push(entry); // source-account credentials, covered by the tx signature
      continue;
    }
    const creds = entry.credentials().address();
    const who = Address.fromScAddress(creds.address()).toString();
    if (who !== cfg.account) {
      log("skipping auth entry for", who);
      signed.push(entry);
      continue;
    }
    creds.signatureExpirationLedger(validUntil);

    const preimage = xdr.HashIdPreimage.envelopeTypeSorobanAuthorization(
      new xdr.HashIdPreimageSorobanAuthorization({
        networkId,
        nonce: creds.nonce(),
        signatureExpirationLedger: validUntil,
        invocation: entry.rootInvocation(),
      }),
    );
    const payload = hash(preimage.toXDR());
    const authDigest = hash(concat(payload, ruleIds.toXDR()));
    log("signature_payload:", hex(payload));
    log("auth_digest (WebAuthn challenge):", hex(authDigest));

    const a = await passkeyAssert(authDigest, cfg.credId, cfg.rpId);
    log("clientDataJSON:", new TextDecoder().decode(a.clientDataJSON));
    log("authenticatorData:", hex(a.authenticatorData), "flags=0x" + a.authenticatorData[32].toString(16));
    log("signature r||s:", hex(a.rawSig));

    const sigData = webauthnSigData(a.authenticatorData, a.clientDataJSON, a.rawSig).toXDR();
    creds.signature(authPayload(signerScVal, sigData, ruleIds));
    signed.push(entry);
  }

  // ---- 3. rebuild with signed auth, simulate, assemble, send ------------
  source = await server.getAccount(payer.publicKey());
  const tx2 = new TransactionBuilder(source, { fee: "1000000", networkPassphrase: cfg.passphrase })
    .addOperation(buildOp(signed))
    .setTimeout(300)
    .build();
  const sim2 = await server.simulateTransaction(tx2);
  if (rpc.Api.isSimulationError(sim2)) {
    log("simulate #2 failed. Error(Contract, #3114) = challenge mismatch; ExternalVerificationFailed = verifier rejected the signature.");
    throw new Error(sim2.error);
  }
  log("simulation with signature OK, submitting…");
  const prepared = rpc.assembleTransaction(tx2, sim2).build();
  prepared.sign(payer);
  const sent = await server.sendTransaction(prepared);
  if (sent.status === "ERROR") throw new Error("send failed: " + JSON.stringify(sent.errorResult));
  log("tx hash:", sent.hash);
  const result = await pollTx(server, sent.hash);
  log("status:", result.status);
  if (result.status === "SUCCESS") {
    log("new signer id:", scValToNative(result.returnValue));
    log(`https://stellar.expert/explorer/testnet/tx/${sent.hash}`);
  }
}

$("run").addEventListener("click", () => run().catch((e) => log("ERROR:", e.message ?? String(e))));
