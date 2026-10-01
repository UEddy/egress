# Exitline

Entry for the Arbitrum Open House Singapore Online Buildathon (HackQuest).
Submission closes Oct 4, 2026, 15:59 as shown on HackQuest (assumed UTC, so 16:59 Lagos). Confirm on the logged-in page.
Registration closes Oct 2, 2026.

## What it is

Exitline protects people who lend money against tokenized stocks. It checks onchain how much of a
stock could actually be sold in a crash. If loans grow bigger than the market can absorb, it
automatically stops new lending on that stock before lenders are left with losses.

Target chain: Robinhood Chain mainnet (chain id 4663, Arbitrum Orbit, Stylus supported).
Loan asset: USDG (Paxos). Judges give extra consideration for USDG.

## Architecture

- `engine/` Rust Arbitrum Stylus contract. `sellProceeds(pool, stockIsToken0, maxImpactBps)` walks a
  Uniswap V3 pool's initialized ticks like a swap would and returns the quote-token amount a
  seller receives before the price falls by `maxImpactBps`. View only, no storage, never swaps.
- `contracts/` Solidity 0.8.28 (Foundry).
  - `ExitlineGuard.sol`: installed as a Morpho **Vault V2 sentinel**. Vault V2 lets a sentinel lower
    caps instantly but never raise them (raises go through the curator's timelock). The guard cuts
    the per-stock cap id `keccak256(abi.encode("collateralToken", stock))`, which covers every
    market for that stock in the vault (MorphoMarketV1AdapterV2.ids()).
  - Keepers record readings at least `minGap` apart. Once 5 readings exist, the cap is cut to the
    **median** reading if lower than the current cap. Median means one or two manipulated readings
    can neither trigger nor block a cut.
  - Recording is keeper-gated on purpose: pool liquidity can be added and removed inside one
    transaction, so permissionless recording would let an attacker fill the window.
  - `emergencyCut` is permissionless: cuts the cap to 0 while an ERC-8056 corporate action
    (`newUIMultiplier != uiMultiplier`, effective within the window) is pending. Uses only the
    functions documented by Robinhood (`uiMultiplier`, `newUIMultiplier`, `effectiveAt`).
  - Fails safe: an unreadable pool counts as zero depth. The closed-market haircut applies on the
    weekend calendar (Sat 00:00 to Mon 01:00 UTC, covering Fri 20:00 to Sun 20:00 New York time
    under both EDT and EST) and, as a fallback, when the feed is unreadable or older than
    `maxOracleAge` (configure enforces >= 26 h so the feeds' 24 h heartbeat never trips it on a
    quiet weekday). `isWeekendClosed(ts)` and `marketClosed(stock)` expose the decision.
  - Known limit: US market holidays (and early closes) are not in the calendar. On a holiday the
    market counts as open unless the feed fallback trips, which with a 26 h floor it usually won't
    for a one-day holiday.
  - `GuardFactory.sol`: deploys a guard for any Vault V2 vault. Deploying grants nothing until the
    vault owner calls `setIsSentinel(guard, true)`.
- `tools/measure/` Rust CLI (reuses `engine::walk` over JSON-RPC, all reads pinned to one block).
  Per Denar stock: USDG borrowed (interest accrued like Morpho), vault cap, collateral, and USDG
  sellable across every Uniswap V3 stock/USDG fee tier within 5/10/20%. `--verify` runs each pool's
  real `swap()` to the same limit inside `eth_call` (runtime code of `probe/SwapProbe.sol` injected
  by state override) and fails unless the walk matches to the wei. `--json PATH` writes the report.
  Also reports stock `balanceOf` held by every lending contract in `lenders.json` (roots with sources;
  aTokens, vaults, loan/margin accounts, gearing tokens derived onchain), valued at the deepest
  pool's spot price. Stocks and the lender scan run on parallel threads, all pinned to one block.
  `markets <TOKEN>` lists every canonical Blue market with TOKEN as collateral: params, accrued
  totals, borrower count, suppliers classified via the Vault V2 / adapter factory registries, and a
  check that collateral over all positions equals `token.balanceOf(Blue)`.
  JSON output never contains individual addresses: borrowers, non-vault suppliers and per-borrower
  contracts (Arcadia accounts, Gage loan accounts, Turret offer vaults) are left out.
- `crosscheck/` Solidity 0.7.6 project with real Uniswap v3-core. Builds pools, runs real swaps to
  price limits, writes `engine/fixtures/*.json`. The Rust tests replay them and require the engine
  to equal the real swap output **to the wei**.

## Status (Oct 1, 2026)

Done and passing:
- Guard + factory: 51 Foundry tests against Morpho's real Vault V2 code (pinned submodule), incl. fuzz.
  Engine gas budget (Sep 29, constants reset Oct 2 from the mainnet measurement below): each engine
  call gets exactly `engineGasLimit` (immutable, set via the factory, bounded to 100k..6.5M,
  recommended 6.1M); `record`/`preview` revert `InsufficientGas` unless gasleft() covers
  every pool's budget + 1/63 + call cost + `RECORD_GAS_OVERHEAD` (250k), so a failed pool always
  failed within its full budget. Keepers size calls with `gasRequired(stock)`. Worst case at the
  minimum accepted gas (every pool burning its full budget, cut through real Vault V2) leaves
  ~195k unused. `StepEngine` (test/mocks) burns steps × gasPerStep, calibrated to the measured
  16,500 gas/step with the engine's 256-step bound as MAX_STEPS.
  Weekend calendar (Oct 1): replaces the oracle-age-only closed test; edge tests at Fri 15:00 and
  23:59:59 UTC, Sat 00:00, Sun 23:59:59, Mon 00:00/00:30/00:59:59/01:00 UTC, both EDT and EST weeks;
  fuzz checks every week has exactly 49 closed hours.
- Fork simulation (Sep 29, `contracts/test/fork/NetNetAaplFork.t.sol` via
  `contracts/script/fork-netnet-aapl.sh`): guard on the real NetNet Credit Vault V2 (owner
  impersonated on the fork only), mock engine returning tools/measure's AAPL depth at the fork
  block. At block 75821799 the AAPL cap went 600,000 -> 68,336 USDG after 5 readings (median, 50%
  of ~137k depth within 5%); the guard could not raise it; the curator restored it after the 3-day
  timelock. The new cap is below the current AAPL allocation (197k): a cut only blocks new lending.
- Engine: 22 Rust tests. TickMath matches v3-core on 16 ticks; 24 real-swap cases match exactly.
  MAX_STEPS is 256 (NVDA/USDG needed 194 at 20%). Square root is integer-only: `ruint`'s `root()`
  uses f64, which Stylus rejects at activation. Toolchain pinned to Rust 1.91.0.
- `cargo stylus check` passes on Robinhood Chain mainnet (Sep 28): 23,076 bytes compressed (under
  24 KB, one fragment), 73,536 uncompressed, activation data fee 0.000120 ETH (incl. 20% bump).
- Engine ABI export verified: `sellProceeds(address,bool,uint256) view returns (uint256)`.
- `tools/measure` (Sep 27): run at block 74157078, snapshot in `tools/measure/results/`. 60 walks
  equal the real swap to the wei; 3 are a pool pinned at MAX_SQRT_RATIO-1 (depth 0, swap cannot move).
  Finding: Denar's 6 equity markets (NVDA, AAPL, MSFT, TSLA, SPY, QQQ; one each) have 20 to 317 USDG
  borrowed and a 10,000 USDG vault cap each. Depth within 5% is 190k (AAPL) to 2.8M (NVDA) USDG, so
  borrows are 600x+ below depth today and caps 19x+ below it. The guard would not cut anything now.
  The onchain engine's 128-step bound undercounts NVDA at 20% by ~2.5% (194 steps needed).
- Lender inventory (block 74197325, `results/denar-74197325.json`): 273 holder contracts across 9
  protocols hold ~$660k of the six stocks. Canonical Morpho Blue holds ~$609k of it (AAPL $301k,
  NVDA $299k); then Native Credit Pool $22k, Gage $15k, Ripe $8k, TermMax $2.7k, Denar $2k,
  LayerBank $84, Arcadia $9, Turret 0. Matches DefiLlama except where DefiLlama unwraps Uniswap LP
  NFTs held as collateral (Arcadia, Gage): that stock sits in the pool, not the lender. Not covered:
  Zona, Oter, Kyros (no public addresses, < $30), Spine (collateral is PT-NVDA, not NVDA).
  AAPL held by all lenders ($301k) exceeds AAPL sellable within 5% (~195k USDG).
- AAPL on canonical Blue (block 74741332, `results/aapl-markets-74741332.json`): 15 of 286 markets
  take AAPL, all lending USDG; one matters. Market 0xdeb4782d012d5fd3b24962538c2f6559049d70bda4dabd2e4212dacb96c28d45
  (LLTV 62.5%, oracle 0xD625d488D552775D2867194C618B945E5dDfE097): 215,037 USDG supplied, 101,052
  borrowed (47% utilization), 5 borrowers, 882.77 AAPL collateral. Nearly all of it is one position
  at 33.7% LTV that liquidates after a 46% AAPL fall. The only supplier is the NetNet Credit Vault V2.
  Other AAPL markets hold at most 25 USDG. No MetaMorpho V1 vault supplies any AAPL market.
  `AAPL.balanceOf(Blue)` exceeds all positions by exactly 0.016527821027914048 AAPL: 80 direct
  transfers from contract 0x39adb8acd07427d338b5f1afab436a04abfdb7c4 with no Blue event (purpose
  unconfirmed; looks like dividend-style payouts). No position owns it.

- ENGINE DEPLOYED to Robinhood Chain mainnet (Oct 2, 2026).
  Address 0x276F4933f06B77912384D64291E062885C33E031, deployer 0xd2B33234991B1B671Df496d6ec3c5B788b0326e7.
  Deploy tx 0x4ffa561c77f827b85288af8226d13cf4676fdce219f06e7e0caf4a7554e16136 (block 77745782,
  5,058,794 gas incl. 19,779 for L1, at 0.035828 gwei) and activation tx
  0xab729bd454111490f6703346e17b07326b8338d7c1c866ecbe849453816c2f56 (block 77745814, 4,500,553 gas
  at 0.035754 gwei, value 0.000119713291997331 ETH of which ArbWasm refunded the 20% bump, so the
  data fee actually paid was 0.000099761076664443 ETH).
  Total spend 0.000441920320058443 ETH against a 0.05 gwei fee cap whose worst case was
  0.000601872191997331 ETH. ArbWasm reports stylusVersion 3, programVersion 3, programInitGas 26,692,
  programMemoryFootprint 17 pages.
  Deployed by `--wasm-file` from a container build, because of the cargo-stylus keystore limitation
  recorded in the next entry. The consequences are worth knowing:
  - the container build is NOT byte-identical to a local build: container wasm 73,680 bytes
    (sha256 dc7cc13a147f5534b8e559fc4cd32707969c2d459c51c5ea9ae84fbb6864504f) vs local 73,536
    (sha256 980bd5b169de6590fb8deb702ae5055715e20ec4d4bd4c9a6310dd85e6a84109). The difference is
    expected: the embedded build paths differ (/source vs /home/...).
  - `--wasm-file` deploys the raw wasm WITHOUT the project metadata hash a project build appends,
    so the compressed size is 23,055 bytes rather than the container project build's 23,123 (a
    local project build is 23,076). The deployed code therefore carries no embedded source hash,
    which is what `cargo stylus verify` keys off. Verified instead by byte comparison: on-chain
    `eth_getCode` is 23,055 bytes with the eff00000 prefix and sha256
    56d052ed92a38b0b32afa03336d301692ce93329027bbbf07c4da744625b70bf, identical to the bytes
    cargo-stylus simulated for activation from the container wasm.
  - the container writes its artifacts as root, so build it on a copy outside the repo or it
    leaves root-owned files in engine/target/.
- cargo-stylus 0.10.9 traps, both hit while preparing the deploy and both still present:
  - `--estimate-gas` prints the activation data fee (in wei) into the "deployment tx gas" field and
    multiplies it by the gas price, so it reported a total of "11961 ETH" at 0.1 gwei and
    "5985 ETH" at 0.05. The figure is meaningless and drifts run to run with the L1 price. The real
    numbers came from the `eth_estimateGas` it sends (captured by pointing `--endpoint` at a local
    logging proxy) and, for activation, from replaying its own simulation (an eth_call with the
    contract's code as a state override on ArbWasm `activateProgram`) through `eth_estimateGas`
    instead of `eth_call`.
  - the reproducible (Docker) path mounts only the workspace root at /source and re-runs itself in
    the container with the same arguments, so a keystore outside the repo is invisible inside and
    the command dies with a bare "No such file or directory (os error 2)". Nothing is wrong with the
    toolchain image (`docker run ... cargo stylus --version` works). To deploy reproducibly and
    verifiably in one step the key material would have to be mounted into the container; that was
    rejected here, hence the `--wasm-file` route above.
  The Stylus deployer at 0xcEcba2F1DC234f70Dd89F2041029807F8D03A990 has no code on this chain. That
  is harmless for the engine (no constructor, hence a plain CREATE) but rules out any
  `--constructor-args` deployment.

- Deployed-engine cross-check (Oct 2, block 77749579, `tools/measure/results/engine-crosscheck-77749579.json`):
  all 63 pool x impact cases (21 Uniswap V3 stock/USDG pools, 500/1000/2000 bps, 15 of them depth 0)
  equal the deployed engine's `sellProceeds` TO THE WEI, zero mismatches, and `--verify` confirmed
  every complete walk equals the pool's real swap. Nothing was step-bounded at those impacts.
- Engine gas on mainnet (Oct 2, measured on the DEPLOYED engine by eth_estimateGas, blocks
  77752815-77753816). Worst case is always NVDA/USDG fee 500 (0xd4EB21209C4D6093f80B5b84f5C45cc093EA14a3):
  3,290,900 gas at 2000 bps (209 steps, completes), 4,065,590 at 5000 bps and 4,065,559 at 9999 bps.
  At 5000 bps the full walk needs 277 steps and at 9999 bps 327, so the engine really does stop at
  its 256-step bound there: 4,065,590 IS a true 256-step walk. Next highest pools are far cheaper
  (AAPL fee 500 1,543,002 at 5000 bps, QQQ fee 500 1,283,641). The 209-to-256 step delta implies
  roughly 16,500 gas per step, which is the figure to calibrate StepEngine's gasPerStep from.
- Gas design set from that measurement (Oct 2, 2026). The three constants were chosen together:
  - MAX_IMPACT_BPS 5,000 -> **2,000**. Depth past a 20% price fall does not describe risk anyone
    acts on, and it is where the walk gets expensive (3,290,900 gas at 20% vs 4,065,590 at 50%).
    Production config uses 500 (5%), so this is well above what is actually configured.
  - MAX_ENGINE_GAS 3,500,000 -> **6,500,000**, with the recommended `engineGasLimit` documented as
    **6,100,000**. The engine's 256-step bound caps ANY walk at about 4,065,590 gas no matter how
    dense the pool, so 6.1M is that measured ceiling plus 50%. The constant sits above it so a
    denser market later does not require a new guard.
  - MAX_POOLS 8 -> **3**. `gasRequired` is n x (budget + 1/63 + ENGINE_CALL_GAS) +
    RECORD_GAS_OVERHEAD, so a full measurement costs about 18.87M at the recommended 6.1M budget
    and about 20.09M at the 6.5M ceiling. Both are under the 32M per-transaction limit of Arbitrum
    Nitro chains, but THAT 32M IS STILL AN ASSUMPTION about Robinhood Chain, not a confirmed
    figure: the block header reports 2^50 (1,125,899,906,842,624), which says nothing useful, and
    no per-transaction limit has been read from the chain or its docs. MAX_POOLS is the constant to
    lower if the real limit turns out smaller. Note AAPL has exactly 3 Uniswap V3 USDG pools, so
    the fork simulation sits exactly at the new ceiling; a fourth pool would not fit.
  StepEngine (test/mocks) now carries MAINNET_GAS_PER_STEP = 16,500 and MAX_STEPS = 256, calibrated
  from the 209-to-256 step delta above rather than guessed, and
  `test_worstCaseFitsInTransactionGasLimit` asserts the design against the measured figures instead
  of only against the constants. A new test,
  `test_calibratedFullStepWalkFitsRecommendedBudget`, drives the calibrated mock for a full 256-step
  walk and requires it to fit the 6.1M budget and count as real depth (it burns ~11.4M gas in the
  test). 51 Foundry tests pass, plus 5 crosscheck, 22 engine and the fork simulation.

Not done (in order):
2. Confirm Robinhood Chain's per-transaction gas limit. The gas constants are now set (see above)
   on the assumption of Nitro's 32M, which has not been verified on this chain. If it is lower,
   lower MAX_POOLS to match. Everything else about the gas design is measured, not assumed.
3. Deliberately NOT deploying GuardFactory, a demo Vault V2 or a guard on mainnet (decided Oct 1 to
   save gas): together they cost ~9.5M gas, more than the remaining balance affords, and the guard
   is already demonstrated end to end by the fork simulation above. Revisit only on a top-up.
4. Dashboard (deployed frontend, required by the submission form).
5. Outreach: ask Vault V2 curators to add the guard as a sentinel. Candidates: Denar dnUSDG2 and
   the four Vault V2 stock vaults below; NetNet Credit first (sole lender to the main AAPL market).

Stretch: sentinel `deallocate`; Uniswap V4 pools via StateView.

## Addresses (Robinhood Chain 4663)

| Contract | Address | Source |
|---|---|---|
| **Exitline depth engine (Stylus)** | **0x276F4933f06B77912384D64291E062885C33E031** | deployed Oct 2, 2026 |
| Morpho Blue (canonical) | 0x9D53d5E3bd5E8d4Cbfa6DB1ca238AEA02E651010 | morpho-org/sdks |
| Vault V2 factory | 0x0FBad98595b0186dA120E41f77C102beb49f803c | morpho-org/sdks |
| MorphoMarketV1AdapterV2 factory | 0x79370Ed003CE325C088E530d5e8655c99c2993e1 | morpho-org/sdks |
| Uniswap V3 factory | 0x1f7d7550b1b028f7571e69a784071f0205fd2efa | Uniswap/sdks |
| Uniswap V4 StateView | 0xf3334192d15450cdd385c8b70e03f9a6bd9e673b | Uniswap/sdks |
| WETH | 0x0Bd7D308f8E1639FAb988df18A8011f41EAcAD73 | Robinhood docs |
| NVDA | 0xd0601CE157Db5bdC3162BbaC2a2C8aF5320D9EEC | Robinscan, verified |
| NVDA/USDG Uniswap V3 pool (largest, ~$5.7M) | 0xd4EB21209C4D6093f80B5b84f5C45cc093EA14a3 | DexScreener |
| Denar equity Morpho Blue (separate instance) | 0xf0A0a33729270586cDD66010B1cedE649745c3A5 | Denar docs |
| Denar USDG vault (MetaMorpho V1, equity) | 0xF0E6AD006080c48766ddb95b8c568D72bC059050 | Denar docs |
| Denar dnUSDG2 (Vault V2, canonical Blue) | 0x338b2f252dae1deb00Afb700128e592a19F8918c | Denar docs |

Vault V2 vaults supplying AAPL markets on canonical Blue (all registered in the Vault V2 factory;
names are self-chosen by each vault, curators are onchain addresses only, Morpho's API lists none):

| Vault V2 | Address | Curator | Owner |
|---|---|---|---|
| NetNet Credit | 0x99347d5F70D3838763f6Bddcf80304C8aa953B57 | 0x3Bb7A23316f82C0e984fA2E784846d8928a35f42 | same as curator |
| Sharewoods RWA USDG | 0x5FE15021a7C0Ff4A9965b400E474f616451BA128 | 0x374ba83b81167176450FEF1a4F07a9eC1D21dB54 | 0x6f75b9102435352235Dd7f4E1ACff4Ea4d5F2631 |
| Longbow Core USDG | 0x026df18fbd2A7639089D0a16293383ec687A5Ca1 | 0xe600452658762042749eb8e11955542B7EBeA4Bb | 0x396ae0BD5623c3750e15fd222770F1e972153ED4 |
| Galaxy USDG Stock Tokens | 0xd0dACE70434fF7567b108B5a8c2a36C150C830FE | 0xA13094eCEd689b7419BAbA12851374424c1486f1 | 0x42D510eDeb9257f8D920d5B9f5109D95cB22419d |

Other Morpho contracts (morpho-org/sdks): MorphoVaultV1Adapter factory 0x7a91222F3f7B927bB8fb624593Ca86e111C2F85e.
There is no official MetaMorpho V1 factory on Robinhood Chain; V1 vaults are identified by interface.

Stock tokens (addresses first seen in a third-party crate): AAPL 0xaF3D76f1834A1d425780943C99Ea8A608f8a93f9,
MSFT 0xe93237C50D904957Cf27E7B1133b510C669c2e74, QQQ 0xD5f3879160bc7c32ebb4dC785F8a4F505888de68,
SPY 0x117cc2133c37B721F49dE2A7a74833232B3B4C0C, TSLA 0x322F0929c4625eD5bAd873c95208D54E1c003b2d.
The five addresses above are the collateral of Denar's equity markets and their `symbol()` matches
onchain (checked by `tools/measure`). USDG is 0x5fc5360D0400a0Fd4f2af552ADD042D716F1d168 (6 decimals),
token0 of the NVDA/USDG pool, symbol checked.

Chainlink feeds on Robinhood Chain (verified Sep 29, 2026). Source: Chainlink's feed directory
(reference-data-directory.vercel.app/feeds-robinhood-mainnet.json, the data behind docs.chain.link),
matched by proxy address; onchain each proxy points at an aggregator reporting "DualAggregator 1.0.0",
all four proxies have owner 0xeE27D5Ae494300902D90454e8630A3F1C68c9C52. All 8 decimals, heartbeat
86400 s, deviation 0.5%. Stock feeds are category "custom", market hours us_equities_24/5.

| Feed (directory name) | Proxy | Onchain description | Used by |
|---|---|---|---|
| Robinhood AAPL / USD | 0x6B22A786bAa607d76728168703a39Ea9C99f2cD0 | "Robinhood AAPL / USD" | AAPL market 0xdeb4782d… oracle |
| Robinhood NVDA / USD | 0x379EC4f7C378F34a1B47E4F3cbeBCbAC3E8E9F15 | "RHNVDA / USD" | NVDA market 0x8b16891f… oracle |
| Robinhood TSLA / USD | 0x4A1166a659A55625345e9515b32adECea5547C38 | "RHTSLA / USD" | TSLA market 0xb41b34c5… oracle |
| USDG / USD | 0x61B7e5650328764B076A108EFF5fa7282a1B9aD2 | "USDG / USD" | quote feed of all three |

Feeds update on a 0.5% move or every 24 h, so an hours-old stock feed during market hours is normal
(AAPL was 2 h 50 m old at 17:18 UTC on Sep 29). For the guard's `maxOracleAge`, age alone is a weak
"market closed" signal: setting it under the heartbeat flags quiet open markets as closed (the safe
direction: a stricter haircut, never a looser one).

Oracles of the main stock markets on canonical Blue (read at block 75829408):
- TSLA market 0xb41b34c5…: oracle 0xca76875634e0b9759aa6610dc3092e92fcefe46e is a
  MorphoChainlinkOracleV2 (confirmed by the Morpho Chainlink oracle factory
  0xB7c16F6F8cF531447Bf27Ca7220f981E79C9cdF2, morpho-org/sdks): BASE_FEED_1 = TSLA/USD,
  QUOTE_FEED_1 = USDG/USD, no vaults, SCALE_FACTOR 1e24.
- AAPL market 0xdeb4782d… (oracle 0xD625d488D552775D2867194C618B945E5dDfE097) and NVDA market
  0x8b16891f… (oracle 0xed29d310cfa91778a5850538da28ed42234cb78c) use the same custom oracle (identical
  2,563-byte code, not from Morpho's factory): baseFeed() = stock/USD, quoteFeed() = USDG/USD,
  MAX_QUOTE_AGE 90,000 s. Three unnamed getters return 0.8e18, 1.2e18 and 349,200 s; likely USDG
  price bounds and a max base age (~4 days), not confirmed.

RPC. $ROBINHOOD_RPC_URL is now an Alchemy endpoint (chain id 4663 confirmed) and it IS an archive
node: reads 500,000 blocks back succeed, so pinned-block work no longer has to race a state window.
It throttles bursts instead: tools/measure's batches of 20 drew HTTP 429 under load, and so did
single calls for a while afterwards. Space requests out and retry on 429.
The public endpoint https://rpc.mainnet.chain.robinhood.com still works (batches of 20 pass, 100 do
not) but is not archive: it keeps state for roughly 4,000-8,000 blocks (~7-13 min), so a pinned-block
run against it must finish inside that window. It also returns HTTP 403 to clients that do not send a
browser-like User-Agent (python-urllib's default is refused; cast and reqwest are fine).
A `--verify` run with lenders takes ~4.5 min; with --no-lenders about 55 s.
Since Sep 29 eth_getLogs ranges are capped at 10M blocks; tools/measure splits them.

Denar's equity vault is MetaMorpho V1, which has no sentinel role. Exitline supports Vault V2 only.
Do not design around holding a curator role.

## Commands (WSL Ubuntu)

```bash
git submodule update --init --recursive
cd contracts && forge test
cd ../crosscheck && forge test          # regenerates engine/fixtures (solc 0.7.6)
cd ../engine && cargo test
rustup target add wasm32-unknown-unknown && cargo install cargo-stylus
cargo stylus check --endpoint "$ROBINHOOD_RPC_URL"
cargo run --features export-abi         # prints the Solidity ABI
cd ../tools/measure && cargo run --release -- --verify --json results/denar-<block>.json
cargo run --release -- markets AAPL --json results/aapl-markets-<block>.json
cargo run --release -- --only AAPL --impacts 500 --no-lenders   # one stock, one bound, fast
cd ../.. && contracts/script/fork-netnet-aapl.sh   # measure + fork test at the same block (~30 s)
# how the deployed engine was built and shipped (see the status notes before reusing this):
#   build reproducibly on a copy outside the repo, since the container writes artifacts as root
#   docker run --rm --network host --workdir /source --volume <copy>:/source \
#     cargo-stylus-base-0.10.9-toolchain-1.91.0 cargo stylus check --endpoint "$ROBINHOOD_RPC_URL"
#   cargo stylus deploy --no-verify --wasm-file <copy>/target/wasm32-unknown-unknown/release/exitline_engine.wasm \
#     --endpoint "$ROBINHOOD_RPC_URL" --keystore-path ~/.foundry/keystores/exitline-deployer \
#     --keystore-password-path <password file> --max-fee-per-gas-gwei 0.05
cast call 0x276F4933f06B77912384D64291E062885C33E031 \
  'sellProceeds(address,bool,uint256)(uint256)' <pool> <stockIsToken0> <bps> --rpc-url "$ROBINHOOD_RPC_URL"
# regenerate probe/SwapProbe.runtime.hex: solc 0.8.28, optimizer 200 runs, evm cancun, deployedBytecode
```

## Rules for this repo

- Security first. The most conservative accurate answer, not the convenient one.
- Every rounding choice must make reported depth smaller, never larger.
- Verify addresses and facts against primary sources before relying on them.
- Fresh deployer key used only for this project. Never commit keys or `.env`.
- No Claude attribution lines in commits.
- Never commit an individual's address (borrowers, non-vault suppliers, per-borrower accounts), in
  CLAUDE.md, results or anywhere else. Protocol contracts, vaults and curators are fine.
- All code is written inside the buildathon window (opened Sep 14, 2026). Keep history honest.
