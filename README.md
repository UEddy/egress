# Egress

Egress protects people who lend money against tokenized stocks. It measures, onchain, how much of
a stock could actually be sold in a crash, and it automatically stops new lending on that stock
before the loans outgrow the market that has to absorb them.

Built for the Arbitrum Open House Singapore Online Buildathon. Target chain is Robinhood Chain
mainnet (chain id 4663, an Arbitrum Orbit chain with Stylus). The loan asset is USDG.

## The problem, in measured numbers

Tokenized equities on Robinhood Chain are already used as loan collateral. The collateral is only
worth what it can be sold for, and that is a far smaller number than the quoted price.

Everything below was measured onchain by `tools/measure`, with every read pinned to a single block.

At block 74197325, lending contracts across nine protocols held roughly $660,000 of the six
Robinhood equity tokens (NVDA, AAPL, MSFT, TSLA, SPY, QQQ), spread over 273 holder contracts.
Canonical Morpho Blue held about $609,000 of that, with AAPL at about $301,000 and NVDA at about
$299,000. The rest sat in Native Credit Pool ($22,000), Gage ($15,000), Ripe ($8,000), TermMax
($2,700), Denar ($2,000), LayerBank ($84) and Arcadia ($9). The totals agree with DefiLlama except
where DefiLlama unwraps Uniswap LP positions pledged as collateral, in which case the stock sits in
the pool rather than with the lender.

The gap that matters: AAPL held by lenders came to about $301,000, while the AAPL that could
actually be sold into Uniswap V3 for USDG before the price fell 5 percent was about $195,000 at the
same block. More AAPL is pledged as collateral than the market can absorb at a 5 percent
concession. Liquidators facing that position do not get the quoted price, and the shortfall lands on
lenders.

The concentration is just as stark. Of 286 canonical Morpho Blue markets, 15 accept AAPL and only
one carries real size: market `0xdeb4782d012d5fd3b24962538c2f6559049d70bda4dabd2e4212dacb96c28d45`
at 62.5 percent LLTV, with 215,037 USDG supplied, 101,052 USDG borrowed and 882.77 AAPL of
collateral. Nearly all of the borrowing is a single position that liquidates after a 46 percent fall
in AAPL. Every other AAPL market holds at most 25 USDG. The sole supplier is one Morpho Vault V2
vault, NetNet Credit. One vault, one market, one position, against depth that cannot clear it.

Depth also moves. At block 77749579 the same measurement put AAPL sellable within 5 percent at
122,242 USDG and NVDA at 1,515,389 USDG. A number measured once is not a safety property, which is
why Egress measures continuously.

## What Egress does

Egress installs as a **sentinel** on a Morpho Vault V2 vault. Vault V2 lets a sentinel lower
supply caps immediately, while any raise has to go through the curator's timelock. That asymmetry is
the whole design: the guard can take risk off the table instantly and can never add any.

Keepers record depth readings. Once five readings exist, the guard cuts the vault's per stock
supply cap to the **median** reading, if the median is below the current cap. The cap id it cuts is
`keccak256(abi.encode("collateralToken", stock))`, which covers every market in the vault that takes
that stock as collateral.

A cut blocks new lending. It never touches existing positions and never moves anyone's funds.

## How it works

**`engine/`** is an Arbitrum Stylus contract in Rust.
`sellProceeds(pool, stockIsToken0, maxImpactBps)` walks a Uniswap V3 pool's initialized ticks the
way a real swap would, and returns the quote token amount a seller receives before the price falls
by `maxImpactBps`. It is view only, holds no storage and never swaps. Every rounding choice makes
reported depth smaller, never larger.

**`contracts/EgressGuard.sol`** is the sentinel. It reads depth from the engine for each
configured pool, applies a coverage factor and a closed market haircut, keeps a rolling window of
readings, and cuts the cap to the median of five.

**`contracts/GuardFactory.sol`** deploys a guard for any Vault V2 vault. Deploying grants nothing:
the guard is inert until the vault owner calls `setIsSentinel(guard, true)`.

**`tools/measure/`** is the measurement CLI. It reuses the same `engine::walk` code over JSON RPC and
reports borrowed amounts, caps, collateral and sellable depth per stock, plus the chain wide lender
inventory above. With `--verify` it replays each pool's real `swap()` to the same price limit inside
`eth_call` and fails unless the walk matches to the wei.

**`crosscheck/`** builds real Uniswap v3-core pools, runs real swaps to price limits, and writes
fixtures the Rust tests replay.

## Security model

The guard is built so that the worst thing a compromised keeper, a manipulated pool or an
unreachable oracle can do is make lending more conservative.

- **It can only lower caps.** Vault V2 gives a sentinel no power to raise a cap, and no power over
  funds. The guard cannot move, borrow, withdraw or reallocate anything.
- **Median of five.** Readings must be at least `minGap` apart, and the cut uses the median of five.
  One or two manipulated readings can neither trigger a cut nor block one.
- **Recording is keeper gated on purpose.** Pool liquidity can be added and removed inside a single
  transaction, so permissionless recording would let an attacker fill the reading window with
  readings of their own making.
- **Fixed gas budget per call.** Each engine call gets exactly `engineGasLimit`, set once as an
  immutable. `record` and `preview` revert with `InsufficientGas` unless the remaining gas covers
  every pool's budget plus the 1/63 reserve, the call cost and a fixed overhead. A pool that fails
  therefore failed inside its full budget, rather than because the transaction was starved.
- **An unreadable pool counts as zero depth.** Failing to read is treated as no liquidity, which can
  only cut harder.
- **Closed market haircut on a calendar, not on a guess.** The haircut applies from Saturday 00:00
  to Monday 01:00 UTC, which covers Friday 20:00 to Sunday 20:00 New York time under both EDT and
  EST. An unreadable or stale feed is a fallback trigger, not the primary signal, because feed age
  alone is a weak indicator: the Chainlink feeds update on a 0.5 percent move or every 24 hours, so
  an hours old price during open market hours is normal. `configure` enforces `maxOracleAge` of at
  least 26 hours so the 24 hour heartbeat never trips the fallback on a quiet weekday.
- **Permissionless emergency cut.** Anyone may call `emergencyCut` to take a cap to zero while an
  ERC-8056 corporate action is pending, using only the functions Robinhood documents
  (`uiMultiplier`, `newUIMultiplier`, `effectiveAt`).

A cut is also not a loss event for existing borrowers. In the fork simulation below, the AAPL cap
falls to roughly 62,000 to 68,000 USDG depending on the block, while the vault's existing AAPL
allocation is about 197,000 USDG. The cut stops new lending and leaves current positions alone.

## Deployed

| What | Where |
|---|---|
| Depth engine (Stylus), Robinhood Chain mainnet | `0x276F4933f06B77912384D64291E062885C33E031` |

Activated at Stylus version 3.

### Verifying the deployed bytes

The Rust crate in `engine/` is still named `exitline-engine`, from the project's earlier name, and
it stays that way deliberately. The deployed contract is verified by comparing hashes against a
rebuild of that crate, and the crate name is baked into the artifact path and into the build itself.
Renaming it would change the bytes and break the check below, so the engine keeps its original name
while the rest of the project is Egress.

Verification is **by byte comparison**, not by `cargo stylus verify`. The engine was built
reproducibly in cargo-stylus's own Docker image and then deployed with `--wasm-file`, which ships the
raw wasm without the project metadata hash that `cargo stylus verify` keys off. So the check is
direct: rebuild in the container and compare hashes.

```bash
# build reproducibly. Do this on a copy of engine/ outside the repo: the container runs as root and
# will otherwise leave root-owned artifacts in engine/target/.
cp -r engine /tmp/engine-repro && rm -rf /tmp/engine-repro/target
docker run --rm --network host --workdir /source --volume /tmp/engine-repro:/source   cargo-stylus-base-0.10.9-toolchain-1.91.0   cargo stylus check --endpoint https://rpc.mainnet.chain.robinhood.com

sha256sum /tmp/engine-repro/target/wasm32-unknown-unknown/release/exitline_engine.wasm
# dc7cc13a147f5534b8e559fc4cd32707969c2d459c51c5ea9ae84fbb6864504f  (73,680 bytes)
```

That image is built by cargo-stylus itself from `offchainlabs/cargo-stylus-base:0.10.9` plus the
Rust toolchain pinned in `engine/rust-toolchain.toml` (1.91.0); any `cargo stylus` invocation without
`--no-verify` creates it.

The deployed contract is the brotli-compressed form of that wasm behind Stylus's `0xeff00000`
prefix, 23,055 bytes in total:

```bash
cast code 0x276F4933f06B77912384D64291E062885C33E031   --rpc-url https://rpc.mainnet.chain.robinhood.com | tail -c +3 | xxd -r -p | sha256sum
# 56d052ed92a38b0b32afa03336d301692ce93329027bbbf07c4da744625b70bf  (23,055 bytes)
```

A local build is deliberately **not** expected to match the container one (73,536 bytes, sha256
`980bd5b1...4109`): the two embed different build paths. Only the container build reproduces the
deployed bytes.

Try it against the largest NVDA/USDG pool, where USDG is token0 so the stock is token1:

```bash
cast call 0x276F4933f06B77912384D64291E062885C33E031 \
  'sellProceeds(address,bool,uint256)(uint256)' \
  0xd4EB21209C4D6093f80B5b84f5C45cc093EA14a3 false 500 \
  --rpc-url https://rpc.mainnet.chain.robinhood.com
```

The result is in USDG units, which has 6 decimals.

The guard and factory are **not** deployed. See the known limits below.

## Verifying and rerunning

```bash
git submodule update --init --recursive

cd contracts && forge test        # 50 tests against Morpho's real Vault V2 code, including fuzz
cd ../crosscheck && forge test     # real Uniswap v3-core swaps, regenerates engine/fixtures
cd ../engine && cargo test         # 22 tests, replays those fixtures and must match to the wei
```

The engine is checked against reality in two directions. `crosscheck/` runs real swaps in real
v3-core pools and the Rust tests require the engine to reproduce them exactly. `tools/measure
--verify` does the same against live mainnet pools by injecting a probe's runtime code with a state
override and running the pool's own `swap()` inside `eth_call`.

Measure live depth and the lender inventory:

```bash
cd tools/measure
cargo run --release -- --verify --json results/denar-<block>.json
cargo run --release -- markets AAPL --json results/aapl-markets-<block>.json
cargo run --release -- --only AAPL --impacts 500 --no-lenders    # one stock, one bound, fast
```

Rerun the mainnet fork simulation, which is how the guard is demonstrated end to end:

```bash
contracts/script/fork-netnet-aapl.sh     # measures and runs the fork test at the same block
```

That test puts the guard on the real NetNet Credit Vault V2, with the vault owner impersonated on
the fork only, and feeds it the AAPL depth that `tools/measure` read at the same block. By default it
forks the current block, so the exact numbers move with the market; set `FORK_BLOCK` to pin one.

Two runs, for a sense of the shape. At block 75821799 the AAPL cap went from 600,000 to 68,336 USDG,
the median reading at 50 percent coverage of roughly 137,000 USDG of depth within 5 percent. At block
77762358 the same cap went to 61,753 USDG against 123,507 USDG of depth. In both, the vault's
existing allocation to AAPL was about 197,000 USDG, well above the new cap, so the cut stopped new
lending without touching open positions. Each run then confirms the guard cannot raise the cap back,
and that the curator restores it only after the three day timelock.

## Dashboard

A static site in `web/` (Vite, React, TypeScript, viem) showing the measured position: total stock
collateral held by lending contracts, the AAPL gap, and every stock's lent amount against sellable
depth at 5, 10 and 20 percent, flagged where lending exceeds depth.

It renders from `web/public/snapshot.json`, which is committed, so the page draws with no network
request and states the block each number was measured at. That file is composed from measured runs
held in `tools/measure/results`, and the page labels every section with its own block: depth and
pools from one run, the lender inventory from another, the AAPL markets from a third. Where
holdings are compared against depth, both figures come from the same run, so the comparison is at
one block. Each stock also has a **Check live** button
that calls the deployed engine's `sellProceeds` at 5 percent on that stock's pools straight from the
browser and prints the live total beside the snapshot value. Those calls happen on click only, one
stock at a time, and go to Robinhood Chain's public endpoint. The frontend reads no environment
variable and contains no private RPC URL: a static bundle is readable by anyone who loads it.

```bash
cd web
npm install
npm run build      # typecheck, then a static build into web/dist
npm run preview    # serve the build locally
npm run dev        # or the dev server while editing
```

It is built to stay quick and calm. The data is inlined at build time, so the first paint needs no
network round trip. Only viem's ABI codec is imported, not its client, which keeps the JavaScript at
67 KB gzipped. Inter is self hosted as one variable woff2, subset to latin, preloaded, with
font-display swap. Motion is limited to transform and opacity so it stays on the compositor:
sections fade and rise 8px as they enter the viewport, staggered 60ms, and the headline figures
count up once. Anyone who asks for reduced motion gets none of it, and smooth scrolling turns off
with it. The table becomes stacked cards on narrow screens rather than scrolling sideways, every
control is at least 44px, and focus is always visible.

Lighthouse, mobile preset: performance 99, accessibility 100, best practices 100, SEO 100, with
cumulative layout shift of 0.

Refresh the data at a new block:

```bash
cd tools/measure
cargo run --release -- snapshot --from-results results
```

That composes `web/public/snapshot.json` offline from the reports in `tools/measure/results`, with
no network access at all, and prints which block each section came from.

There is also a single pinned block form, `cargo run --release -- snapshot`, which reads state from
`$ROBINHOOD_RPC_URL` and sends only `eth_getLogs` to the public endpoint. It does not currently
finish on free RPC tiers: the public endpoint takes wide log ranges but prunes state after a few
thousand blocks, while the archive endpoint holds state but caps `eth_getLogs` at a ten block range
and throttles sustained `eth_call`. One endpoint that allows both would make it work. Until then the
composed file is what the site ships, and the Check live button is what proves the numbers current.

The build output is a plain static directory, so any static host serves it. On Vercel the Vite preset
needs no configuration beyond the `web` root directory.

## Known limits

Stated plainly, because a risk tool that oversells itself is worse than none.

- **The guard is not deployed on mainnet.** Only the engine is live. The guard, the factory and a
  demo vault were costed at roughly 9.5 million gas and skipped to stay inside the deployer's
  balance. The guard is demonstrated by the fork simulation against real Vault V2 code, not by a
  live installation.
- **Robinhood Chain's per transaction gas limit is still unconfirmed.** The guard's gas constants
  are set from the mainnet measurement: depth may be measured to at most a 20 percent price fall
  (`MAX_IMPACT_BPS` 2000), a per pool engine budget of at most 6,500,000 (`MAX_ENGINE_GAS`), and at
  most 3 pools per stock (`MAX_POOLS`). The recommended `engineGasLimit` is 6,100,000, which is the
  measured 256 step cost of 4,065,590 plus 50 percent. A full measurement therefore costs about
  18.9 million gas at that budget and about 20.1 million at the ceiling, both under the 32 million
  per transaction limit of Arbitrum Nitro chains. That 32 million is an assumption about this chain,
  not a confirmed figure, which is why `MAX_POOLS` is deliberately small.
- **US market holidays are not in the calendar.** Only weekends are. On a holiday the market counts
  as open unless the stale feed fallback trips, which with a 26 hour floor it usually will not for a
  single day closure.
- **Uniswap V3 pools only.** Uniswap V4 liquidity is not counted, so reported depth is a lower bound
  and the guard is more conservative than reality. Reading V4 pools through StateView is a planned
  extension.
- **The engine's walk is bounded at 256 steps.** In a pool dense enough to need more, the engine
  reports less depth than exists. That direction is deliberate.
- **Vault V2 only.** MetaMorpho V1 has no sentinel role, so vaults like Denar's equity vault cannot
  use Egress without granting a curator role, which this design deliberately avoids.
- **Depth is a snapshot of one side of the book.** Sellable totals assume every pool is sold into up
  to the bound at the same time, and no other seller is competing.

## Layout

```
engine/        Rust Stylus contract, the depth engine (view only)
contracts/     Solidity 0.8.28, the sentinel guard and its factory (Foundry)
crosscheck/    Solidity 0.7.6 with real Uniswap v3-core, generates test fixtures
tools/measure/ Rust CLI for onchain depth, lender inventory, market listings and the snapshot
web/           Dashboard: static Vite, React and TypeScript site, reads the snapshot
```
