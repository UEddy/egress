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
  - Fails safe: an unreadable pool counts as zero depth; an unreadable or stale oracle applies the
    closed-market haircut.
  - `GuardFactory.sol`: deploys a guard for any Vault V2 vault. Deploying grants nothing until the
    vault owner calls `setIsSentinel(guard, true)`.
- `crosscheck/` Solidity 0.7.6 project with real Uniswap v3-core. Builds pools, runs real swaps to
  price limits, writes `engine/fixtures/*.json`. The Rust tests replay them and require the engine
  to equal the real swap output **to the wei**.

## Status (Sep 27, 2026)

Done and passing:
- Guard + factory: 31 Foundry tests against Morpho's real Vault V2 code (pinned submodule), incl. fuzz.
- Engine: 20 Rust tests. TickMath matches v3-core on 16 ticks; 24 real-swap cases match exactly.
- Engine ABI export verified: `sellProceeds(address,bool,uint256) view returns (uint256)`.

Not done (in order):
1. `tools/measure`: local Rust CLI reusing `engine` walk over JSON-RPC. For each Denar stock market:
   total USDG borrowed vs USDG sellable within 5/10/20%. Collateral total per Morpho Blue instance =
   `stock.balanceOf(morphoBlue)`. Denar's equity vault is MetaMorpho V1: read `supplyQueue` for market
   ids, then Blue `idToMarketParams` and `market`. Verify every token's `symbol()` onchain. Discover
   USDG from the NVDA/USDG pool's token0/token1. This is the PMF evidence: do it before building more.
2. Build the engine to wasm and `cargo stylus check` against Robinhood Chain.
3. Deploy engine, factory, a demo Vault V2 (via the official factory) and a guard on mainnet.
4. Dashboard (deployed frontend, required by the submission form).
5. Outreach to Denar: ask them to add the guard as a sentinel on their Vault V2 vault.

Stretch: sentinel `deallocate`; Uniswap V4 pools via StateView.

## Addresses (Robinhood Chain 4663)

| Contract | Address | Source |
|---|---|---|
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

Unverified (from a third-party crate, check with `cast` before use): AAPL 0xaF3D76f1834A1d425780943C99Ea8A608f8a93f9,
MSFT 0xe93237C50D904957Cf27E7B1133b510C669c2e74, QQQ 0xD5f3879160bc7c32ebb4dC785F8a4F505888de68,
SPY 0x117cc2133c37B721F49dE2A7a74833232B3B4C0C, TSLA 0x322F0929c4625eD5bAd873c95208D54E1c003b2d.
USDG address: not yet confirmed. Chainlink stock feed addresses: not yet confirmed.

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
```

## Rules for this repo

- Security first. The most conservative accurate answer, not the convenient one.
- Every rounding choice must make reported depth smaller, never larger.
- Verify addresses and facts against primary sources before relying on them.
- Fresh deployer key used only for this project. Never commit keys or `.env`.
- No Claude attribution lines in commits.
- All code is written inside the buildathon window (opened Sep 14, 2026). Keep history honest.
