# hemiProve — Hemi's Decentralized ZK Proving Marketplace

Research notes on hemiProve and what it would take for Strait to index it. Compiled 2026-11-11.

> **Source confidence.** This doc mixes three kinds of information. Each section says which kind it holds.
>
> | Tag | Meaning |
> |---|---|
> | **[repo]** | Read from the public `hemilabs/zk-miner` source (README and `crates/zkminer-contracts/src/bindings.rs`). This is the most reliable source here, but it describes what the **miner** expects, not the deployed contracts. |
> | **[announced]** | From Hemi's announcement and social posts (Binance Square and similar), relayed through prior research. I have **not** independently confirmed the announcement: a web search on 2026-11-11 did not find it. |
> | **[proposal]** | Strait's own product ideas. Not Hemi's plans. |
>
> Before any indexer code is written, check event signatures against the **deployed** ABI on Hemi testnet (chain 743111). The signatures below come from an automated extraction of `bindings.rs`, so re-read that file directly when implementing.

---

## 1. What hemiProve is

hemiProve is a **marketplace for zero-knowledge proofs**. Applications that need a computation proven post jobs. Independent operators ("provers"/"miners") with GPUs claim them, generate the proof, submit it on-chain, and get paid. Provers back each claim with staked collateral.

It is separate from Hemi's other security mechanism:

| Mechanism | What it does |
|---|---|
| **Proof-of-Proof (PoP)** | Anchors Hemi state into Bitcoin for Bitcoin-backed finality. See [pop-anchoring.md](pop-anchoring.md). |
| **ZK proofs** | Prove that a computation ran correctly, so the verifier doesn't have to repeat it. |
| **hemiProve** | A decentralized market that matches proof requests with provers who generate them. |

PoP secures the chain. hemiProve produces proofs that applications and, later, the tunnels can verify.

### Status **[announced]** + **[repo]**

| Item | Status |
|---|---|
| Announcement | Reported as 2026-11-05 **[announced, unverified]** |
| Public miner (`hemilabs/zk-miner`, Rust, MIT) | Public, actively updated **[repo]** |
| RISC Zero / SP1 / OpenVM backends | Implemented in the miner **[repo]** |
| NVIDIA / AMD / Intel GPU workers | Implemented in the miner **[repo]** |
| Testnet config (chain 743111) | In the README; `run` checks contract code against the live testnet deployment **[repo]** |
| Public beta open to everyone | Described as "forthcoming" **[announced]** |
| Ethereum Tunnel ZK settlement | Planned, not live **[announced]** |
| hBitVM for the Bitcoin Tunnel | Planned, after the Ethereum Tunnel **[announced]** |
| macOS prover support | "Coming"; Windows and Linux only for now **[announced]** |

---

## 2. Actors

| Actor | Role |
|---|---|
| **Requester / caller** | Submits a job: program, inputs, price range, deadline. Deposits the payment. May name a callback contract to receive the result. |
| **Prover** | Stakes tHEMI/HEMI, claims jobs, generates proofs, calls `fulfillJob`. |
| **Operator** | An address that can claim or fulfil on behalf of a prover (`claimJobDelegated`; the `operator` field appears in events). |
| **Keeper / slash caller** | Anyone who slashes a prover that missed its deadline and reopens the job, for a keeper reward (`slashAndReopen`, `slashAndClaim`). |
| **Governance** | Registers proof-system adapters and programs, and can pause the protocol and disable adapters. |

---

## 3. Contracts **[repo]**

The miner's config names five contracts. The README shows only `0x…` placeholders for their addresses. Get the real ones from the miner's built-in "known deployment" defaults or the Hemi explorer, and record them in [contract-addresses.md](contract-addresses.md) once confirmed.

| Config key | Contract | Notes |
|---|---|---|
| `hemi_prove` | **HemiProve router** | A diamond proxy. Facets: `IHemiProveCore`, `IHemiProveFulfill`, `IHemiProveAux`. Emits all job lifecycle events. |
| `hemi_prove_staking` | **Staking** | Prover stake, locked collateral, unstake queue. |
| `hemi_prove_registry` | **Registry** | Proof-system adapters (`getAdapter(proofSystemId)`), blacklist, pause flag. |
| `hemi_token` | **HEMI / tHEMI** | ERC-20 used for staking. Testnet tHEMI has a `mint`. |
| `program_registry` | **ProgramRegistry** | Program ID → ELF hash, storage URIs, resource estimate. |

Verifier adapters (`IVerifierAdapter`) wrap each zkVM's native verifier: RISC Zero (`IRiscZeroSelectable.SELECTOR`) and SP1 (`ISP1NativeVerifier.VERIFIER_HASH`). The miner pins their bytecode as a safety check.

| Network | Chain ID | RPC |
|---|---|---|
| Hemi testnet | 743111 | `https://testnet.rpc.hemi.network/rpc` |
| Hemi mainnet | 43111 | — (no hemiProve deployment referenced) |

---

## 4. Job lifecycle

### 4.1 State machine

```
               submitJob*
                   │
                   ▼
   ┌────────── OPEN ◄───────────────────────────────┐
   │   (price ramps up within [minPrice,maxPrice])  │
   │               │ claimJob / claimJobDelegated   │
   │ cancelJob     ▼                                │
   │           CLAIMED (collateral locked,          │
   │                    lockDeadline set)           │
   │      ┌────────┼─────────────┬──────────────┐   │
   │      │        │             │              │   │
   │ fulfillJob  releaseJob   deadline missed   grace(Pause)Release
   │      │     (5% penalty)  → slashAndReopen  (no penalty;
   │      ▼        │          or slashAndClaim   protocol paused /
   │  FULFILLED    └──────────────┴──────────────┴───┘ adapter disabled)
   ▼
CANCELLED
```

- **Submit.** The requester calls `submitJob` (or `submitJobDefault`, `submitJobSimple`, `submitJobBatch`), deposits funds, and sets `minPrice`, `maxPrice`, `fulfillmentTimeout` and a `trust` level. Emits `JobSubmitted`.
- **Price discovery.** A rising-price (Dutch-style ascending) auction: the offered price ramps from `minPrice` toward `maxPrice` until a prover claims the job. `getCurrentPrice`, `suggestPriceRange` and `getPremiumCap` expose it. A competitive-bid mode (`submitBid`, `settleBid`, `BidState`, `CompetitiveBidConfig`) also exists.
- **Exclusivity.** A job can name an `assignedProver`. Other provers can claim it only after an exclusivity window.
- **Claim.** `claimJob(jobId, priceFloor, premiumFloor)` locks the prover's collateral and returns the settled price. It reverts with `ClaimBelowFloor` if the price moved below the prover's floor. Emits `JobClaimed` with the `lockDeadline`.
- **Fulfil.** `fulfillJob` / `fulfillJobHashed` submits the proof. The router calls the verifier adapter and checks the journal against `expectedJournalHash` / `journalFields` (outcomes: satisfied, fails, none, unchecked). On success it pays the prover, refunds the requester, takes a protocol fee, and may pay a speed bonus. Emits `JobFulfilled` and `PayoutCredited`.
- **Release.** The prover gives up voluntarily (`releaseJob`) and pays **5% of the locked collateral**. The job returns to OPEN with a higher bonus. Emits `JobReleased`.
- **Slash.** After `lockDeadline`, anyone can call `slashAndReopen` (or `slashAndClaim` to slash and take the job in one call). The prover's collateral is slashed, part goes to the keeper as a reward and part is burned, and the job reopens. Emits `JobReopened`. If no one slashes, the collateral stays locked indefinitely.
- **Grace release.** If the protocol is paused or the job's adapter is disabled, `gracePauseRelease` / `graceRelease` returns collateral without penalty.
- **Cancel.** The requester calls `cancelJob`. Emits `JobCancelled` with the refund and the burned bonus.
- **Deadline extension.** `DeadlineExtended` moves `lockDeadline`.

### 4.2 Events (indexer source of truth) **[repo — verify against deployed ABI]**

```solidity
// Router — IHemiProveCore
event JobSubmitted(bytes32 indexed jobId, bytes32 indexed proofSystemId, address indexed caller,
                   bytes32 programId, bytes32 descriptorHash, uint96 depositedAmount,
                   uint96 minPrice, uint96 maxPrice, uint40 fulfillmentTimeout, uint8 trust);
event JobClaimed(bytes32 indexed jobId, address indexed prover, address indexed operator,
                 uint96 settledPrice, uint96 bonusAmount, uint40 lockDeadline);
event PayoutCredited(address indexed recipient, bytes32 jobId, uint256 amount);

// Router — IHemiProveFulfill
event JobFulfilled(bytes32 indexed jobId, address indexed prover, address indexed caller,
                   uint96 settledPrice, uint96 bonusAmount, uint256 proverPayout,
                   uint256 callerRefund, uint256 protocolFee, uint256 speedBonus,
                   uint64 actualCycles, bytes publicValues);
event JobReleased(bytes32 indexed jobId, address indexed prover, uint256 penaltyAmount,
                  uint96 newBonusAmount, uint256 timeHeld, address operator);
event JobReopened(bytes32 indexed jobId, address indexed slashedProver, address indexed slashCaller,
                  uint128 slashedAmount, uint256 keeperReward, uint256 burnedAmount,
                  uint96 newBonusAmount, uint8 reopenCount, uint40 newRampUpStart,
                  uint40 elapsedAtLock, address operator);
event JobCancelled(bytes32 indexed jobId, address indexed caller, uint256 refundAmount, uint256 bonusBurned);
event GracePauseReleased(bytes32 indexed jobId, address indexed prover, uint128 releasedCollateral, address operator);
event GraceReleased(bytes32 indexed jobId, address indexed prover, bytes32 proofSystemId,
                    uint128 releasedCollateral, address operator);
event DeadlineExtended(bytes32 indexed jobId, uint40 extension, uint40 lockDeadline);

// Router — IHemiProveAux
event Paused(address account);
event Unpaused(address account);

// Staking
event Staked(address indexed prover, uint128 amount, uint128 newTotal);
event UnstakeRequested(address indexed prover, uint128 amount, uint40 withdrawAfter);
event Withdrawn(address indexed prover, uint128 amount, uint128 newTotal);
event StakeReduced(address indexed prover, uint128 amount, uint128 newTotal,
                   uint128 newPendingUnstake, bool slashed);

// Registry
event AdapterDisabledEvent(bytes32 indexed proofSystemId);
```

Gotchas for indexing:

- **No `JobSlashed` event.** Slashing appears only through `JobReopened` (`slashedProver`, `slashedAmount`) and `StakeReduced(slashed = true)`.
- **No `ProgramRegistered` event.** `ProgramRegistry.registerProgram` emits nothing, so programs must be discovered by calling `getProgram(programId)` for each `programId` seen in `JobSubmitted`.
- **`actualCycles` is always 0** under the current governance adapters. Don't build "cycles proven" metrics on it.
- **The job descriptor isn't in the event.** `JobSubmitted` carries only `descriptorHash`. The full `JobDescriptor` (inputs, callback, `assignedProver`, `tag`) is in the submit transaction's calldata, so decode it from the transaction if you need it.
- **A job can be claimed several times.** Release and reopen send it back to OPEN, so model claims as a child table, not a column on `jobs`. `reopenCount` is a `uint8`.
- `getJobStatusView(jobId)` returns a 22-field snapshot, which is useful for reconciliation and backfill checks.

### 4.3 Key structs

```solidity
struct JobDescriptor {
    bytes32 programId;          // must equal the ELF image ID (risc0) or vkey hash (SP1)
    bytes32 proofSystemId;      // selects the verifier adapter
    address callbackContract;   // optional result receiver
    address assignedProver;     // optional exclusivity
    bytes32 tag;                // requester-defined label (candidate for tunnel correlation)
    bytes   inputData;
    bytes   callbackExtraData;
    bytes   extraVerifierData;
    bytes32 expectedJournalHash;
    bytes32 journalFields;
}

struct ProverStake {
    uint128 totalStaked;
    uint128 lockedCollateral;   // available = totalStaked - lockedCollateral
    uint128 unstakeAmount;
    uint40  unstakeRequestTime;
    uint40  depositBlock;
}
```

`ProgramVersion` (from `ProgramRegistry.getProgram`) holds `elf_hash`, `StorageURI[]` with content hashes, and a `ResourceEstimate`.

---

## 5. Economics **[repo]**

- **Collateral:** available = `totalStaked − lockedCollateral` (`getAvailableCollateral(prover)`). A claim locks collateral sized to the job; the test suite uses **500% of job value**. The miner refuses jobs whose collateral exceeds the prover's entire stake.
- **Penalties:**

  | Event | Cost |
  |---|---|
  | Voluntary release | 5% of the lock's collateral |
  | Missed deadline | Slashed, split between keeper reward and burn |
  | Missed deadline, nobody slashes | Collateral stays locked |

- **Payout split on fulfil:** `proverPayout`, `callerRefund` (unused deposit above the settled price), `protocolFee`, `speedBonus`, plus any `bonusAmount` built up from earlier releases or reopens.
- **Unstaking** has a delay: `requestUnstake`, then wait until `withdrawAfter`, then `withdrawUnstaked`.

---

## 6. The miner (`hemilabs/zk-miner`) **[repo]**

Strait shouldn't run this, but its design explains what happens on-chain.

| Crate | Responsibility |
|---|---|
| `zkminer-cli` | `init`, `status`, `benchmark [--calibrate]`, `run [--watch-only] [--headless]`, `simulate` |
| `zkminer-chain` | RPC client, auction math, staking, job monitor, tx/nonce handling, RPC metering |
| `zkminer-contracts` | `sol!()` bindings: the miner's view of the ABI |
| `zkminer-prover` | Worker dispatcher, ELF registry, benchmarks |
| `zkminer-prover-protocol` | IPC between dispatcher and workers |
| `zkminer-prove-risc0` / `-sp1` / `-openvm` | One GPU worker process per GPU, so a crash stays isolated |
| `zkminer-strategy` | Profitability, cost model, timing prediction |
| `zkminer-tui` | Terminal UI (ratatui) |
| `zkminer-config` | `~/.zkminer/config.toml`, key loading (`ZKMINER_PRIVATE_KEY`) |

**zkVM × GPU support:**

| | NVIDIA (CUDA 12.8–13.x) | AMD (ROCm 6.4+) | Intel (oneAPI/SYCL) |
|---|---|---|---|
| RISC Zero | ✅ | ✅ | ✅ |
| SP1 | ✅ | optional | optional |
| OpenVM | ✅ (worker binary) | — | — |

**Hardware notes:** RISC Zero segment size (po2) runs 16–22. po2 20 peaks at about 12.1 GiB VRAM, so cards with less than 14 GiB get no default. The RISC Zero Groth16 component needs about 5 GB and the SP1 Groth16 artifacts about 8 GB. Rust is pinned to 1.93.0.

**`--watch-only` is useful to Strait.** It watches the marketplace and shows what the miner *would* claim, without signing anything. Running it on testnet is a cheap way to watch real jobs while we design the schema.

**The miner's safety checks double as an integrity checklist:** program ID matches the ELF hash, descriptor hash matches, journal binding holds, adapter and verifier bytecode are pinned, and the deployment address matches the expected one. The miner's `simulate` outcome labels (`program_mismatch`, `too_heavy`, `no_program`, `timeout`, and others) are a good vocabulary for job-health classification.

---

## 7. Tunnel roadmap **[announced]**

1. hemiProve marketplace live (beta)
2. **Ethereum Tunnel ZK settlement.** Today Hemi→ETH withdrawals settle through the OP-stack path (see [eth-tunnel-guide.md](eth-tunnel-guide.md) and Strait's L1 `WithdrawalFinalized` settlement). ZK settlement could shorten or replace the challenge window.
3. **hBitVM for the Bitcoin Tunnel.** BitVM-backed custody for high-value vaults, alongside the existing overcollateralized vaults ([btc-tunnel-guide.md](btc-tunnel-guide.md)).
4. Open marketplace for third-party proof requests.

**Open question:** how tunnel settlements will map to proving jobs. Hemi hasn't documented it. Likely candidates are a dedicated `programId`, a known `caller` or `callbackContract` (the tunnel's settlement contract), or the descriptor's `tag`. **Don't assume one job per withdrawal.** Settlement proofs will probably batch many withdrawals per job.

---

## 8. What Strait could build **[proposal]**

Recommended direction: **add hemiProve as a new module in Strait** instead of starting a separate product, and **start with job indexing and a Proof Explorer**. Don't build a miner.

### Phase 1: job indexing (first milestone)

1. Confirm the router, staking, registry and program-registry addresses on testnet 743111, then add them to [contract-addresses.md](contract-addresses.md).
2. Pull the deployed ABI, diff it against §4.2, and record the deployment block.
3. Add a `hemiprove` watcher to the existing Hemi indexer. The router and staking contracts are both on Hemi, so the existing RPC and backfill machinery should mostly carry over.
4. Proposed schema:

   | Table | Key | Populated from |
   |---|---|---|
   | `prove_jobs` | `job_id` | `JobSubmitted`, then status updates from every lifecycle event |
   | `prove_job_claims` | (`job_id`, `claim_seq`) | `JobClaimed`, closed by `JobFulfilled` / `JobReleased` / `JobReopened` / `Grace*` |
   | `prove_payouts` | (`tx`, `log_index`) | `PayoutCredited`, `JobFulfilled` split |
   | `provers` | `address` | `Staked`, `Withdrawn`, `StakeReduced`, `UnstakeRequested`, plus `getProverStats` |
   | `prove_programs` | `program_id` | Lazy `ProgramRegistry.getProgram` on first sighting |
   | `prove_protocol_events` | (`tx`, `log_index`) | `Paused`, `Unpaused`, `AdapterDisabledEvent`, `DeadlineExtended` |

5. Derived metrics: time to claim (claim − submit), time to prove (fulfil − claim), settled price vs. `[minPrice, maxPrice]`, release/slash rate, protocol fees, and job counts by `proofSystemId`.
6. API, following [api-integration.md](api-integration.md) conventions: `GET /api/v1/proofs`, `/proofs/:jobId`, `/provers`, `/provers/:address`, `/programs`, `/analytics/proofs/overview`.

### Phase 2: prover analytics

Prover profiles (stake, locked vs. available collateral, jobs completed, release and slash history, median proving time, proof systems used) and a marketplace page (price curves, open-job depth, fee revenue).

### Phase 3: tunnel ↔ proof correlation

Once Hemi ships ETH-tunnel ZK settlement: identify the settlement program or caller, then link `prove_jobs` to the withdrawals already in `Transfer`, and add a "proof settled" step to the withdrawal lifecycle. Later, track hBitVM the same way.

### Phase 4: infrastructure intelligence

Historical exports, webhooks ([webhooks-implementation-plan.md](webhooks-implementation-plan.md)) for job events, and network-health alerts (pause, adapter disabled, slashes spiking).

### Funding note

This fits the Hemi grant ($2,000 upfront, $200/month) without becoming a separate project. For the six-month review, Strait can cover both cross-chain asset movement and ZK proving and settlement.

---

## 9. Implementation plan for the Strait codebase **[proposal]**

This is how Phase 1 fits into the existing workspace. None of it is built yet.

### 9.1 Run it as a separate watcher

[ingester.rs](../crates/strait-evm/src/ingester.rs) already has the pattern hemiProve needs: windowed `get_logs`, dispatch on `topic0`, a checkpoint per window, and a `ReorgDetector`. hemiProve should reuse that pattern but run as its **own task**, not as extra events inside the existing Hemi ingester:

1. **Different network.** hemiProve is deployed on Hemi testnet (743111). The tunnel ingester follows the Hemi network it's configured for, so hemiProve needs its own RPC URL and chain ID in config.
2. **Separate checkpoint.** A shared checkpoint would mean a hemiProve failure could stall or rewind tunnel indexing. The `checkpoints` table (migration 003) is keyed by `chain TEXT`, so a `hemiprove` row fits without a schema change. `CheckpointRepo` takes the `Chain` enum, though, so it needs a string-key variant (or a new enum value).

### 9.2 Where the pieces go

| Piece | Location | Content |
|---|---|---|
| ABI | [contracts.rs](../crates/strait-evm/src/contracts.rs) | New `sol!` block with the router and staking events from §4.2 |
| Schema | `crates/strait-store/migrations/012_hemiprove.sql` | `prove_jobs`, `prove_job_claims`, `provers`, `prove_programs` |
| Repo | `crates/strait-store/src/prove_jobs.rs` | Repository code in the style of `tunnel_transfers.rs` |
| Watcher | `crates/strait-node/src/hemiprove_watcher.rs` | Log loop, checkpoint and reorg handling, modelled on the ingester |
| Wiring | [main.rs](../crates/strait-node/src/main.rs) | Another `tasks.spawn`, gated on hemiProve config being present |
| API | `strait-api` | GraphQL `proofJob` / `proofJobs` / `prover` queries, plus REST equivalents |

### 9.3 Sketches

ABI, taken from the miner's bindings:

```rust
sol! {
    interface IHemiProve {
        event JobSubmitted(bytes32 indexed jobId, bytes32 indexed proofSystemId, address indexed caller,
            bytes32 programId, bytes32 descriptorHash, uint96 depositedAmount,
            uint96 minPrice, uint96 maxPrice, uint40 fulfillmentTimeout, uint8 trust);
        event JobClaimed(bytes32 indexed jobId, address indexed prover, address indexed operator,
            uint96 settledPrice, uint96 bonusAmount, uint40 lockDeadline);
        // JobFulfilled, JobReleased, JobReopened, JobCancelled, Grace*, DeadlineExtended …
    }
}
```

Watcher dispatch, the same shape as the ingester's `process_log`:

```rust
match log.topic0() {
    Some(&IHemiProve::JobSubmitted::SIGNATURE_HASH) => {
        let e = log.log_decode::<IHemiProve::JobSubmitted>()?.inner.data;
        repo.insert_job(&e, block, ts, tx).await?;            // status = OPEN
    }
    Some(&IHemiProve::JobClaimed::SIGNATURE_HASH) => {
        repo.open_claim(job_id, prover, price, lock_deadline).await?;   // status = CLAIMED
    }
    Some(&IHemiProve::JobFulfilled::SIGNATURE_HASH) => {
        repo.close_claim(job_id, ClaimOutcome::Fulfilled).await?;
        repo.set_job_fulfilled(job_id, payout, fee, speed_bonus).await?;
    }
    Some(&IHemiProve::JobReleased::SIGNATURE_HASH)
    | Some(&IHemiProve::JobReopened::SIGNATURE_HASH) => {
        repo.close_claim(job_id, outcome).await?;              // job back to OPEN
    }
    // Staked / StakeReduced / Withdrawn → provers table
    _ => {}
}
```

Core tables:

```sql
CREATE TABLE prove_jobs (
  job_id TEXT PRIMARY KEY, proof_system_id TEXT, program_id TEXT, caller TEXT,
  status TEXT NOT NULL,              -- open | claimed | fulfilled | cancelled
  min_price NUMERIC, max_price NUMERIC, deposited NUMERIC,
  submitted_at TIMESTAMPTZ, submit_tx TEXT, fulfilled_at TIMESTAMPTZ, fulfil_tx TEXT,
  prover_payout NUMERIC, protocol_fee NUMERIC, speed_bonus NUMERIC, reopen_count INT DEFAULT 0
);

CREATE TABLE prove_job_claims (
  job_id TEXT REFERENCES prove_jobs, claim_seq INT, prover TEXT, settled_price NUMERIC,
  claimed_at TIMESTAMPTZ, lock_deadline TIMESTAMPTZ,
  outcome TEXT,                      -- fulfilled | released | slashed | grace_released | NULL (active)
  PRIMARY KEY (job_id, claim_seq)
);
```

Claims get their own table because a job can be claimed, released or slashed, and then claimed again.

### 9.4 Build order and size

1. **Blocker:** get the router and staking addresses and the deployment block on testnet. They aren't in the miner's README; try its built-in deployment defaults or the Hemi explorer.
2. Check the §4.2 event signatures against the deployed ABI.
3. Write the migration, then the repo, then the watcher, then the wiring in `main.rs`. That's about one migration, one repo module, and a watcher of roughly 400 lines, comparable to `payout_watcher.rs`.
4. Add API endpoints as a follow-up.

---

## 10. Open questions to resolve before building

- [ ] Deployed addresses and deployment blocks for router, staking, registry, program registry, and token (testnet; mainnet if any).
- [ ] Does the deployed ABI match the miner's bindings? Check especially `JobSubmitted` field order and `trust` semantics.
- [ ] What the `trust` levels (`uint8`) mean.
- [ ] Which `proofSystemId` values map to RISC Zero, SP1 and OpenVM.
- [ ] Whether OpenVM jobs are live on testnet (the miner's `simulate` doesn't measure them).
- [ ] How tunnel settlement jobs will be identified.
- [ ] Is there an official hemiProve subgraph or API we'd duplicate?
- [ ] Confirm the announcement itself (date, beta timeline) from Hemi's own blog or docs.

## Sources

- `hemilabs/zk-miner`: https://github.com/hemilabs/zk-miner (README; `crates/zkminer-contracts/src/bindings.rs`)
- hemiProve announcement and roadmap: Hemi social channels and Binance Square posts, via prior research (not independently verified)
- Hemi whitepaper (Bitcoin Tunnel dual custody): https://hemi.xyz
