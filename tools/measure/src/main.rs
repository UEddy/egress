//! egress-measure: for each stock market behind Denar's equity vault, compares USDG borrowed
//! against the USDG a seller of the stock could get out of Uniswap V3 before the price falls by
//! 5%, 10% and 20%. Depth comes from the same tick walk the Stylus engine runs onchain.
//!
//! Also: egress-measure markets <TOKEN> [--rpc URL] [--block N] [--json PATH]
//! lists every canonical Morpho Blue market with TOKEN as collateral (see src/markets.rs).
//!
//! Also: egress-measure snapshot [--rpc URL] [--logs-rpc URL] [--block N] [--out PATH]
//! writes the dashboard's data file, by default web/public/snapshot.json (see src/snapshot.rs).
//! State reads go to --rpc and eth_getLogs to --logs-rpc, which defaults to the public endpoint:
//! Alchemy is an archive node but caps log ranges at 10 blocks, and the public endpoint takes any
//! range but prunes state after a few thousand blocks.
//!
//! Usage: egress-measure [--rpc URL] [--block N] [--impacts 500,1000,2000] [--json PATH] [--verify]
//!                         [--lenders PATH | --no-lenders] [--only AAPL,NVDA]
//! The RPC defaults to $ROBINHOOD_RPC_URL, then to Robinhood Chain's public endpoint.
//!
//! It also reports stock tokens held by lending contracts (see lenders.json and src/lenders.rs),
//! valued at the spot price of each stock's deepest Uniswap V3 USDG pool. --no-lenders skips it.
//!
//! --verify also runs each pool's real `swap` to the same price limit inside eth_call (see
//! probe/SwapProbe.sol) and fails unless the walk equals the swap's output to the wei.

mod lenders;
mod markets;
mod rpc;
mod snapshot;
mod verify;

use std::collections::BTreeMap;
use std::process::exit;

use alloy_primitives::{address, Address, B256, U256};
use exitline_engine::math::sqrt_price_limit;
use exitline_engine::walk::{sell_proceeds_bounded, PoolReader, Walk, MAX_STEPS};
use serde::Serialize;

use crate::rpc::*;

const CHAIN_ID: u64 = 4663;
const PUBLIC_RPC: &str = "https://rpc.mainnet.chain.robinhood.com";
const UNIV3_FACTORY: Address = address!("1f7d7550b1b028f7571e69a784071f0205fd2efa");
const NVDA: Address = address!("d0601CE157Db5bdC3162BbaC2a2C8aF5320D9EEC");
const NVDA_USDG_POOL: Address = address!("d4EB21209C4D6093f80B5b84f5C45cc093EA14a3");
const DENAR_BLUE: Address = address!("f0A0a33729270586cDD66010B1cedE649745c3A5");
const DENAR_VAULT: Address = address!("F0E6AD006080c48766ddb95b8c568D72bC059050");
const FEE_TIERS: [u32; 4] = [100, 500, 3000, 10000];
/// Offchain step bound. Far above MAX_STEPS so a capped onchain reading can be seen and sized.
const OFFCHAIN_STEPS: u32 = 20_000;
/// Blocks to stay below the lower of the two endpoints' heads when pinning, so the pinned block is
/// certainly present on both.
const HEAD_MARGIN: u64 = 4;
/// Unused address where --verify places the probe's runtime code for the duration of an eth_call.
const PROBE: Address = address!("00000000000000000000000000000000E71711e0");
/// Runtime code of probe/SwapProbe.sol (solc 0.8.28, optimizer 200 runs, evm cancun).
const PROBE_CODE: &str = include_str!("../probe/SwapProbe.runtime.hex");

/// Stock token addresses listed in CLAUDE.md. A listed address whose onchain symbol differs is a
/// hard error; an unlisted collateral is reported with its onchain symbol and flagged.
const LISTED: [(Address, &str); 6] = [
    (NVDA, "NVDA"),
    (address!("aF3D76f1834A1d425780943C99Ea8A608f8a93f9"), "AAPL"),
    (address!("e93237C50D904957Cf27E7B1133b510C669c2e74"), "MSFT"),
    (address!("D5f3879160bc7c32ebb4dC785F8a4F505888de68"), "QQQ"),
    (address!("117cc2133c37B721F49dE2A7a74833232B3B4C0C"), "SPY"),
    (address!("322F0929c4625eD5bAd873c95208D54E1c003b2d"), "TSLA"),
];


struct Args {
    rpc: String,
    block: Option<u64>,
    impacts: Vec<u64>,
    json: Option<String>,
    verify: bool,
    lenders: Option<String>,
    /// Restrict the depth report to these stocks (listed symbols or addresses).
    only: Option<Vec<String>>,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        rpc: std::env::var("ROBINHOOD_RPC_URL").ok().filter(|s| s.starts_with("http")).unwrap_or(PUBLIC_RPC.into()),
        block: None,
        impacts: vec![500, 1000, 2000],
        json: None,
        verify: false,
        lenders: Some(concat!(env!("CARGO_MANIFEST_DIR"), "/lenders.json").into()),
        only: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut val = || it.next().ok_or(format!("{flag} needs a value"));
        match flag.as_str() {
            "--rpc" => a.rpc = val()?,
            "--block" => a.block = Some(val()?.parse().map_err(|e| format!("--block: {e}"))?),
            "--impacts" => {
                a.impacts = val()?
                    .split(',')
                    .map(|s| s.trim().parse::<u64>().map_err(|e| format!("--impacts: {e}")))
                    .collect::<Result<_, _>>()?;
                if a.impacts.iter().any(|&b| b == 0 || b >= 10_000) {
                    return Err("--impacts: each bound must be in 1..9999 bps".into());
                }
            }
            "--json" => a.json = Some(val()?),
            "--verify" => a.verify = true,
            "--lenders" => a.lenders = Some(val()?),
            "--no-lenders" => a.lenders = None,
            "--only" => a.only = Some(val()?.split(',').map(|s| s.trim().to_string()).collect()),
            "-h" | "--help" => {
                println!(
                    "egress-measure [--rpc URL] [--block N] [--impacts 500,1000,2000] [--json PATH] [--verify] [--lenders PATH | --no-lenders] [--only SYMBOLS]"
                );
                exit(0);
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(a)
}

#[derive(Serialize)]
struct Report {
    chain_id: u64,
    block: u64,
    timestamp: u64,
    usdg: Address,
    morpho_blue: Address,
    vault: Address,
    impacts_bps: Vec<u64>,
    verified: bool,
    stocks: Vec<StockReport>,
    lenders: Option<lenders::LenderReport>,
    notes: Vec<String>,
}

#[derive(Serialize)]
struct MarketReport {
    id: B256,
    oracle: Address,
    irm: Address,
    lltv_wad: String,
    /// The vault's supply cap for this market (MetaMorpho config(id).cap).
    vault_cap: String,
    /// Stored borrow plus interest accrued to the pinned block, as Morpho would compute it.
    borrowed: String,
    /// False when the IRM could not be read and `borrowed` is the stored value without accrual.
    accrued: bool,
    /// Oracle price scaled by 1e36, adjusted for decimals, as Morpho reads it. None if unreadable.
    oracle_price: Option<String>,
}

#[derive(Serialize)]
struct PoolDepth {
    impact_bps: u64,
    /// USDG proceeds from the full offchain walk, rounded down.
    proceeds: String,
    /// False if even the offchain bound was hit: `proceeds` is then a lower bound.
    complete: bool,
    /// What the onchain engine (MAX_STEPS) reports. Smaller than `proceeds` when it was capped.
    engine_proceeds: String,
    engine_complete: bool,
    steps: u32,
    /// With --verify: USDG the pool's real swap paid out to the same limit. Equal to `proceeds`.
    swap_proceeds: Option<String>,
}

#[derive(Serialize)]
struct PoolReport {
    pool: Address,
    fee: u32,
    stock_is_token0: bool,
    depth: Vec<PoolDepth>,
}

#[derive(Serialize)]
struct StockReport {
    stock: Address,
    symbol: String,
    listed: bool,
    decimals: u8,
    markets: Vec<MarketReport>,
    borrowed: String,
    vault_cap: String,
    /// stock.balanceOf(morphoBlue): all collateral of this stock in the Blue instance.
    collateral: String,
    /// Collateral valued at the lowest readable oracle price among this stock's markets.
    collateral_value: Option<String>,
    pools: Vec<PoolReport>,
    /// Sum across pools per impact bound, rounded down per pool.
    sellable: Vec<String>,
    /// The pool with the most depth at the first impact bound; lender holdings are valued at its
    /// spot price.
    price_pool: Option<Address>,
    /// USDG units per whole stock token at that pool's spot price, rounded down.
    price: Option<String>,
    #[serde(skip)]
    spot: Option<(U256, bool)>,
}

fn main() {
    let res = match std::env::args().nth(1).as_deref() {
        Some("markets") => run_markets(),
        Some("snapshot") => run_snapshot(),
        Some("verify-engine") => run_verify_engine(),
        _ => parse_args().and_then(run),
    };
    if let Err(e) = res {
        fail(&e);
    }
}

/// USDG: the non-NVDA side of the NVDA/USDG pool, after checking the pool really is Uniswap's and
/// the token really calls itself USDG. Nothing here trusts a hardcoded address on its own.
fn discover_usdg(rpc: &Rpc) -> Result<(Address, u8), String> {
    let factory = rpc.call(NVDA_USDG_POOL, IUniswapV3Pool::factoryCall {})?;
    ensure(factory == UNIV3_FACTORY, || format!("NVDA/USDG pool factory is {factory}, not Uniswap V3"))?;
    let t0 = rpc.call(NVDA_USDG_POOL, IUniswapV3Pool::token0Call {})?;
    let t1 = rpc.call(NVDA_USDG_POOL, IUniswapV3Pool::token1Call {})?;
    let usdg = match (t0 == NVDA, t1 == NVDA) {
        (true, false) => t1,
        (false, true) => t0,
        _ => return Err(format!("NVDA/USDG pool tokens are {t0}, {t1}")),
    };
    let usdg_symbol = symbol(rpc, usdg)?;
    ensure(usdg_symbol == "USDG", || format!("{usdg} symbol is {usdg_symbol:?}, expected USDG"))?;
    let decimals = rpc.call(usdg, IERC20::decimalsCall {})?;
    eprintln!("USDG {usdg} ({decimals} decimals)");
    Ok((usdg, decimals))
}

/// `snapshot`: writes the dashboard's data file for one pinned block.
fn run_snapshot() -> Result<(), String> {
    let mut rpc_url =
        std::env::var("ROBINHOOD_RPC_URL").ok().filter(|s| s.starts_with("http")).unwrap_or(PUBLIC_RPC.into());
    let mut block = None;
    // Logs default to the public endpoint whatever --rpc is, because it is the only one that
    // accepts a wide block range. Pass --logs-rpc to override.
    let mut logs_rpc: String = PUBLIC_RPC.into();
    let mut out: String = concat!(env!("CARGO_MANIFEST_DIR"), "/../../web/public/snapshot.json").into();
    let mut lenders_path: String = concat!(env!("CARGO_MANIFEST_DIR"), "/lenders.json").into();
    let mut impacts = vec![500u64, 1000, 2000];
    let mut from_results: Option<String> = None;
    let mut depth_file: Option<String> = None;
    let mut lenders_file: Option<String> = None;
    let mut markets_files: Vec<String> = Vec::new();
    let mut it = std::env::args().skip(2);
    while let Some(a) = it.next() {
        let mut val = || it.next().ok_or(format!("{a} needs a value"));
        match a.as_str() {
            "--rpc" => rpc_url = val()?,
            "--logs-rpc" => logs_rpc = val()?,
            "--block" => block = Some(val()?.parse::<u64>().map_err(|e| format!("--block: {e}"))?),
            "--out" => out = val()?,
            "--lenders" => lenders_path = val()?,
            // Offline: assemble the file from reports that already ran, each labelled with its
            // own block. Defaults pick the newest matching report in the directory.
            "--from-results" => from_results = Some(val()?),
            "--depth-file" => depth_file = Some(val()?),
            "--lenders-file" => lenders_file = Some(val()?),
            "--markets-file" => markets_files.push(val()?),
            "--impacts" => {
                impacts = val()?
                    .split(',')
                    .map(|s| s.trim().parse::<u64>().map_err(|e| format!("--impacts: {e}")))
                    .collect::<Result<_, _>>()?;
                if impacts.iter().any(|&b| b == 0 || b >= 10_000) {
                    return Err("--impacts: each bound must be in 1..9999 bps".into());
                }
            }
            "-h" | "--help" => {
                println!(
                    "egress-measure snapshot [--rpc URL] [--logs-rpc URL] [--block N] [--out PATH] [--impacts 500,1000,2000]\n\
                     egress-measure snapshot --from-results DIR [--depth-file F] [--lenders-file F] [--markets-file F ...] [--out PATH]"
                );
                exit(0);
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }

    // Composing from reports that already ran needs no network at all.
    if let Some(dir) = from_results {
        let pick = |pat: &str| -> Result<String, String> {
            let mut hits: Vec<String> = std::fs::read_dir(&dir)
                .map_err(|e| format!("{dir}: {e}"))?
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| n.contains(pat) && n.ends_with(".json"))
                .collect();
            hits.sort();
            hits.pop().map(|n| format!("{dir}/{n}")).ok_or(format!("{dir}: no {pat}*.json"))
        };
        let depth = depth_file.map_or_else(|| pick("engine-crosscheck-"), Ok)?;
        let lend = lenders_file.map_or_else(|| pick("denar-"), Ok)?;
        let mkts = if markets_files.is_empty() {
            std::fs::read_dir(&dir)
                .map_err(|e| format!("{dir}: {e}"))?
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| n.contains("-markets-") && n.ends_with(".json"))
                .map(|n| format!("{dir}/{n}"))
                .collect()
        } else {
            markets_files
        };
        return snapshot::from_results(&depth, &lend, &mkts, &out);
    }

    let mut rpc = Rpc::new(&rpc_url);
    let chain_id = rpc.chain_id()?;
    ensure(chain_id == CHAIN_ID, || format!("chain id {chain_id}, expected {CHAIN_ID}"))?;

    // State reads and log reads can need different endpoints: Alchemy is an archive node but caps
    // eth_getLogs at a 10 block range, while the public endpoint takes any range but prunes state
    // after a few thousand blocks. Pin below both heads so the one block is valid on each.
    if logs_rpc != rpc_url {
        rpc.set_logs_endpoint(&logs_rpc);
        let probe = Rpc::new(&logs_rpc);
        let lid = probe.chain_id()?;
        ensure(lid == CHAIN_ID, || format!("logs endpoint chain id {lid}, expected {CHAIN_ID}"))?;
        if block.is_none() {
            let (state_head, logs_head) = (rpc.head()?, probe.head()?);
            let pin = state_head.min(logs_head).saturating_sub(HEAD_MARGIN);
            eprintln!("state head {state_head}, logs head {logs_head}, pinning {pin}");
            block = Some(pin);
        }
    }

    let (number, timestamp) = rpc.pin_block(block)?;
    eprintln!("chain {chain_id}, block {number} (timestamp {timestamp})");
    eprintln!("  state rpc {}", redact(&rpc_url));
    eprintln!("  logs  rpc {}", redact(rpc.logs_endpoint()));
    let (usdg, usdg_decimals) = discover_usdg(&rpc)?;
    snapshot::run(&mut rpc, chain_id, number, timestamp, usdg, usdg_decimals, &lenders_path, &impacts, &out)
}

/// `verify-engine`: re-asks the deployed engine every reading in a results file.
fn run_verify_engine() -> Result<(), String> {
    let mut rpc_url =
        std::env::var("ROBINHOOD_RPC_URL").ok().filter(|s| s.starts_with("http")).unwrap_or(PUBLIC_RPC.into());
    let mut engine: Address = "0x276F4933f06B77912384D64291E062885C33E031".parse().unwrap();
    let mut path: Option<String> = None;
    let mut it = std::env::args().skip(2);
    while let Some(a) = it.next() {
        let mut val = || it.next().ok_or(format!("{a} needs a value"));
        match a.as_str() {
            "--rpc" => rpc_url = val()?,
            "--engine" => engine = val()?.parse().map_err(|_| "--engine: not an address".to_string())?,
            "-h" | "--help" => {
                println!("egress-measure verify-engine <results.json> [--rpc URL] [--engine ADDR]");
                exit(0);
            }
            p if path.is_none() && !p.starts_with("--") => path = Some(p.to_string()),
            other => return Err(format!("unknown argument {other}")),
        }
    }
    let path = path.ok_or("usage: egress-measure verify-engine <results.json>")?;

    let mut rpc = Rpc::new(&rpc_url);
    let chain_id = rpc.chain_id()?;
    ensure(chain_id == CHAIN_ID, || format!("chain id {chain_id}, expected {CHAIN_ID}"))?;
    eprintln!("engine {engine}, rpc {}", redact(&rpc_url));
    verify::run(&mut rpc, engine, &path)
}

fn run_markets() -> Result<(), String> {
    let mut rpc_url =
        std::env::var("ROBINHOOD_RPC_URL").ok().filter(|s| s.starts_with("http")).unwrap_or(PUBLIC_RPC.into());
    let (mut block, mut json, mut token) = (None, None, None);
    let mut it = std::env::args().skip(2);
    while let Some(a) = it.next() {
        let mut val = || it.next().ok_or(format!("{a} needs a value"));
        match a.as_str() {
            "--rpc" => rpc_url = val()?,
            "--block" => block = Some(val()?.parse::<u64>().map_err(|e| format!("--block: {e}"))?),
            "--json" => json = Some(val()?),
            t if token.is_none() && !t.starts_with("--") => token = Some(t.to_string()),
            other => return Err(format!("unknown argument {other}")),
        }
    }
    let token = token.ok_or("usage: egress-measure markets <TOKEN address or listed symbol>")?;
    let token = match LISTED.iter().find(|(_, s)| s.eq_ignore_ascii_case(&token)) {
        Some((a, _)) => *a,
        None => token.parse::<Address>().map_err(|_| format!("{token} is neither an address nor a listed symbol"))?,
    };

    let mut rpc = Rpc::new(&rpc_url);
    let chain_id = rpc.chain_id()?;
    ensure(chain_id == CHAIN_ID, || format!("chain id {chain_id}, expected {CHAIN_ID}"))?;
    let (number, timestamp) = rpc.pin_block(block)?;
    eprintln!("chain {chain_id}, block {number} (timestamp {timestamp}), rpc {}", redact(&rpc_url));
    let r = markets::run(&rpc, chain_id, number, timestamp, token)?;
    print_markets(&r);
    eprintln!("{} RPC requests", rpc.calls.get());
    if let Some(path) = json {
        let s = serde_json::to_string_pretty(&r).map_err(|e| e.to_string())?;
        std::fs::write(&path, s + "\n").map_err(|e| format!("{path}: {e}"))?;
        eprintln!("wrote {path}");
    }
    Ok(())
}

fn print_markets(r: &markets::MarketsReport) {
    let u = |s: &str| s.parse::<U256>().unwrap();
    let cd = r.collateral_decimals;
    println!();
    println!(
        "{} ({}) as collateral on Morpho Blue {}, block {} (unix {})",
        r.collateral_symbol, r.collateral_token, r.morpho_blue, r.block, r.timestamp
    );
    println!("{} of {} markets. Amounts in the loan token; collateral in {}.", r.markets.len(), r.markets_scanned, r.collateral_symbol);
    println!();
    println!(
        "{:<12} {:<6} {:>6} {:<42} {:>14} {:>14} {:>6} {:>9} {:>12}",
        "MARKET", "LOAN", "LLTV", "ORACLE", "SUPPLIED", "BORROWED", "UTIL", "BORROWERS", "COLLATERAL"
    );
    for m in &r.markets {
        let (sup, bor) = (u(&m.total_supplied), u(&m.total_borrowed));
        let util = if sup.is_zero() { "-".into() } else { format!("{:.1}%", (bor * U256::from(1000u32) / sup).to::<u64>() as f64 / 10.0) };
        println!(
            "{:<12} {:<6} {:>5.1}% {:<42} {:>14} {:>14} {:>6} {:>9} {:>12}",
            format!("{:.10}", m.id.to_string()),
            m.loan_symbol,
            u(&m.lltv_wad).to::<u128>() as f64 / 1e16,
            m.oracle.to_string(),
            units(sup, m.loan_decimals, 2),
            units(bor, m.loan_decimals, 2),
            util,
            m.borrowers.len(),
            units(u(&m.collateral), cd, 4),
        );
    }
    println!();
    println!("Suppliers (current supply positions):");
    for m in &r.markets {
        if m.suppliers.is_empty() {
            continue;
        }
        println!("  {}", m.id);
        for s in &m.suppliers {
            let who = match (s.kind.as_str(), s.vault, &s.vault_name, s.address) {
                ("vault_v2_adapter", Some(v), Some(n), Some(a)) => format!("Vault V2 {v} \"{n}\" via adapter {a}"),
                ("vault_v1", Some(v), Some(n), _) => format!("MetaMorpho V1 {v} \"{n}\""),
                _ => "not a vault (individual supplier)".into(),
            };
            let cur = match (s.curator, s.owner) {
                (Some(c), Some(o)) => format!("  curator {c}, owner {o}"),
                _ => String::new(),
            };
            println!("    {:>14} {}  {who}{cur}", units(u(&s.assets), m.loan_decimals, 2), m.loan_symbol);
            for p in &s.v2_parents {
                println!(
                    "{:>24}held by Vault V2 {} \"{}\" (curator {}, owner {}) via adapter {}{}",
                    "",
                    p.vault,
                    p.name,
                    p.curator,
                    p.owner,
                    p.adapter,
                    if p.enabled { "" } else { " [adapter disabled]" }
                );
            }
        }
    }
    let (bal, found) = (u(&r.blue_balance), u(&r.collateral_found));
    println!();
    println!(
        "Check: {}.balanceOf(Blue) = {}; collateral over all positions found = {}.",
        r.collateral_symbol,
        units(bal, cd, 6),
        units(found, cd, 6),
    );
    if bal > found {
        println!(
            "  Blue holds {} wei more than all positions: tokens sent to Blue directly or lent in a market \
             with this token as the loan asset. No position owns them.",
            bal - found
        );
    } else if bal < found {
        println!("  WARNING: positions claim more than Blue holds ({} wei).", found - bal);
    }
    println!("V1 vaults are identified by interface: Robinhood Chain has no official MetaMorpho V1 factory.");
}

fn fail(msg: &str) -> ! {
    eprintln!("error: {msg}");
    exit(1);
}

fn ensure(cond: bool, msg: impl FnOnce() -> String) -> Result<(), String> {
    if cond {
        Ok(())
    } else {
        Err(msg())
    }
}

fn symbol(rpc: &Rpc, token: Address) -> Result<String, String> {
    rpc.call(token, IERC20::symbolCall {})
}

fn run(args: Args) -> Result<(), String> {
    let mut rpc = Rpc::new(&args.rpc);
    let mut notes = Vec::new();

    let chain_id = rpc.chain_id()?;
    ensure(chain_id == CHAIN_ID, || format!("chain id {chain_id}, expected {CHAIN_ID}"))?;
    let (block, timestamp) = rpc.pin_block(args.block)?;
    eprintln!("chain {chain_id}, block {block} (timestamp {timestamp}), rpc {}", redact(&args.rpc));

    let (usdg, usdg_decimals) = discover_usdg(&rpc)?;

    // Denar equity vault (MetaMorpho V1) on Denar's own Morpho Blue instance.
    let blue = rpc.call(DENAR_VAULT, IMetaMorpho::MORPHOCall {})?;
    ensure(blue == DENAR_BLUE, || format!("vault MORPHO() is {blue}, expected {DENAR_BLUE}"))?;
    let asset = rpc.call(DENAR_VAULT, IMetaMorpho::assetCall {})?;
    ensure(asset == usdg, || format!("vault asset is {asset}, expected USDG {usdg}"))?;

    // Supply queue per CLAUDE.md, plus the withdraw queue, which lists every enabled market
    // including any removed from the supply queue that still carry borrows.
    let mut ids: Vec<B256> = Vec::new();
    let sq = rpc.call(DENAR_VAULT, IMetaMorpho::supplyQueueLengthCall {})?;
    for i in 0..sq.to::<u64>() {
        ids.push(rpc.call(DENAR_VAULT, IMetaMorpho::supplyQueueCall { i: U256::from(i) })?);
    }
    let wq = rpc.call(DENAR_VAULT, IMetaMorpho::withdrawQueueLengthCall {})?;
    for i in 0..wq.to::<u64>() {
        let id = rpc.call(DENAR_VAULT, IMetaMorpho::withdrawQueueCall { i: U256::from(i) })?;
        if !ids.contains(&id) {
            notes.push(format!("market {id} is in the withdraw queue but not the supply queue"));
            ids.push(id);
        }
    }
    eprintln!("vault {DENAR_VAULT}: {} markets", ids.len());

    // Group markets by collateral stock.
    let mut by_stock: BTreeMap<Address, Vec<(B256, MarketParams, Market)>> = BTreeMap::new();
    for id in ids {
        let p = rpc.call(blue, IMorpho::idToMarketParamsCall { id })?;
        if p.collateralToken == Address::ZERO {
            notes.push(format!("market {id} has no collateral (idle market), skipped"));
            continue;
        }
        if p.loanToken != usdg {
            notes.push(format!("market {id} lends {} not USDG, skipped", p.loanToken));
            continue;
        }
        let m = rpc.call(blue, IMorpho::marketCall { id })?;
        by_stock.entry(p.collateralToken).or_default().push((id, p, m));
    }

    if let Some(only) = &args.only {
        let keep = only
            .iter()
            .map(|t| match LISTED.iter().find(|(_, s)| s.eq_ignore_ascii_case(t)) {
                Some((a, _)) => Ok(*a),
                None => t.parse::<Address>().map_err(|_| format!("--only: {t} is neither a listed symbol nor an address")),
            })
            .collect::<Result<Vec<_>, _>>()?;
        by_stock.retain(|a, _| keep.contains(a));
        ensure(!by_stock.is_empty(), || "--only matched none of the vault's stocks".into())?;
    }
    let inventory = args.lenders.as_deref().map(lenders::load).transpose()?;
    let stock_addrs: Vec<Address> = by_stock.keys().copied().collect();

    // Each stock and the lender scan run on their own thread and client, all pinned to the same
    // block. The public endpoint prunes state after roughly ten minutes, so wall time matters.
    let ctx_args = (blue, usdg, timestamp, &args.impacts[..], args.verify);
    let (stock_results, lender_result) = std::thread::scope(|sc| {
        let lender = inventory.as_ref().map(|inv| {
            let r = rpc.fork();
            let addrs = &stock_addrs;
            sc.spawn(move || {
                let mut n = Vec::new();
                let c = lenders::collect(&r, inv, addrs, &mut n);
                (c, n, r.calls.get())
            })
        });
        let per_stock: Vec<_> = by_stock
            .into_iter()
            .map(|(stock, markets)| {
                let r = rpc.fork();
                sc.spawn(move || {
                    let (blue, usdg, now, impacts, verify) = ctx_args;
                    let ctx = Ctx { rpc: &r, blue, usdg, now, impacts, verify };
                    let mut n = Vec::new();
                    let res = measure_stock(&ctx, stock, markets, &mut n);
                    (res, n, r.calls.get())
                })
            })
            .collect();
        let stocks: Vec<_> = per_stock.into_iter().map(|h| h.join().expect("stock thread panicked")).collect();
        (stocks, lender.map(|h| h.join().expect("lender thread panicked")))
    });

    let mut stocks = Vec::new();
    for (res, n, calls) in stock_results {
        rpc.calls.set(rpc.calls.get() + calls);
        notes.extend(n);
        stocks.push(res?);
    }
    let lenders = match (lender_result, &inventory) {
        (Some((collected, n, calls)), Some(inv)) => {
            rpc.calls.set(rpc.calls.get() + calls);
            notes.extend(n);
            let priced: Vec<lenders::Priced> = stocks
                .iter()
                .map(|s| lenders::Priced { address: s.stock, symbol: &s.symbol, spot: s.spot })
                .collect();
            let r = lenders::report(collected?, inv, &priced)?;
            eprintln!("lenders: {} holder contracts checked", r.holders_checked);
            Some(r)
        }
        _ => None,
    };

    notes.push("Uniswap V4 pools are not counted, so sellable depth is a lower bound.".into());
    notes.push("Sellable totals assume each pool is sold into up to the bound at the same time.".into());

    let report = Report {
        chain_id,
        block,
        timestamp,
        usdg,
        morpho_blue: blue,
        vault: DENAR_VAULT,
        impacts_bps: args.impacts.clone(),
        verified: args.verify,
        stocks,
        lenders,
        notes,
    };
    print_table(&report, usdg_decimals);
    eprintln!("{} RPC requests", rpc.calls.get());
    if let Some(path) = &args.json {
        let s = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
        std::fs::write(path, s + "\n").map_err(|e| format!("{path}: {e}"))?;
        eprintln!("wrote {path}");
    }
    Ok(())
}

struct Ctx<'a> {
    rpc: &'a Rpc,
    blue: Address,
    usdg: Address,
    now: u64,
    impacts: &'a [u64],
    verify: bool,
}

fn measure_stock(
    ctx: &Ctx,
    stock: Address,
    markets: Vec<(B256, MarketParams, Market)>,
    notes: &mut Vec<String>,
) -> Result<StockReport, String> {
    let Ctx { rpc, blue, usdg, now, impacts, .. } = *ctx;
    let sym = symbol(rpc, stock)?;
    let listed = match LISTED.iter().find(|(a, _)| *a == stock) {
        Some((_, want)) => {
            ensure(sym == *want, || format!("{stock} symbol is {sym:?}, CLAUDE.md lists {want}"))?;
            true
        }
        None => {
            notes.push(format!("collateral {stock} ({sym}) is not in the listed stock addresses"));
            false
        }
    };
    let decimals = rpc.call(stock, IERC20::decimalsCall {})?;
    let collateral = rpc.call(stock, IERC20::balanceOfCall { who: blue })?;

    let mut borrowed = U256::ZERO;
    let mut vault_cap = U256::ZERO;
    let mut min_price: Option<U256> = None;
    let mut market_reports = Vec::new();
    for (id, p, m) in markets {
        let (b, accrued) = accrued_borrow(rpc, &p, &m, now);
        if !accrued {
            notes.push(format!("{sym} market {id}: IRM unreadable, borrow is the stored value"));
        }
        borrowed += b;
        let cap = U256::from(rpc.call(DENAR_VAULT, IVaultConfig::configCall { id })?.cap);
        vault_cap += cap;
        let price = rpc.call(p.oracle, IOracle::priceCall {}).ok();
        match price {
            Some(px) => min_price = Some(min_price.map_or(px, |q| q.min(px))),
            None => notes.push(format!("{sym} market {id}: oracle {} unreadable", p.oracle)),
        }
        market_reports.push(MarketReport {
            id,
            oracle: p.oracle,
            irm: p.irm,
            lltv_wad: p.lltv.to_string(),
            vault_cap: cap.to_string(),
            borrowed: b.to_string(),
            accrued,
            oracle_price: price.map(|x| x.to_string()),
        });
    }
    let scale = U256::from(10u8).pow(U256::from(36u8));
    let collateral_value = min_price.map(|px| collateral * px / scale);

    let mut pools = Vec::new();
    let mut sellable = vec![U256::ZERO; impacts.len()];
    let mut best: Option<(U256, Address, U256, bool)> = None;
    for fee in FEE_TIERS {
        let pool = rpc.call(UNIV3_FACTORY, IUniswapV3Factory::getPoolCall { a: stock, b: usdg, fee: fee.try_into().unwrap() })?;
        if pool == Address::ZERO {
            continue;
        }
        let t0 = rpc.call(pool, IUniswapV3Pool::token0Call {})?;
        let t1 = rpc.call(pool, IUniswapV3Pool::token1Call {})?;
        ensure((t0, t1) == (stock, usdg) || (t0, t1) == (usdg, stock), || format!("pool {pool} tokens {t0}, {t1}"))?;
        let stock_is_token0 = t0 == stock;
        let reader = RpcPool::new(rpc, pool);
        reader.prefetch(stock_is_token0, impacts.iter().copied().max().unwrap_or(0))?;
        let mut depth = Vec::new();
        for (i, &bps) in impacts.iter().enumerate() {
            let engine = walk(&reader, stock_is_token0, bps, MAX_STEPS, pool)?;
            let full = if engine.complete { engine } else { walk(&reader, stock_is_token0, bps, OFFCHAIN_STEPS, pool)? };
            if !engine.complete {
                notes.push(format!(
                    "{sym} pool {pool} at {bps} bps: onchain engine stops at {MAX_STEPS} steps; full walk took {}",
                    full.steps
                ));
            }
            if !full.complete {
                notes.push(format!("{sym} pool {pool} at {bps} bps: offchain bound hit, depth is a lower bound"));
            }
            let swap_proceeds = if ctx.verify && full.complete {
                let out = real_swap(rpc, &reader, stock_is_token0, bps)
                    .map_err(|e| format!("{sym} pool {pool} at {bps} bps: real swap: {e}"))?;
                ensure(out == full.proceeds, || {
                    format!("{sym} pool {pool} at {bps} bps: walk {} != real swap {out}", full.proceeds)
                })?;
                Some(out.to_string())
            } else {
                None
            };
            sellable[i] += full.proceeds;
            depth.push(PoolDepth {
                impact_bps: bps,
                proceeds: full.proceeds.to_string(),
                complete: full.complete,
                engine_proceeds: engine.proceeds.to_string(),
                engine_complete: engine.complete,
                steps: full.steps,
                swap_proceeds,
            });
        }
        let first = depth[0].proceeds.parse::<U256>().unwrap();
        if !first.is_zero() && best.is_none_or(|(b, ..)| first > b) {
            best = Some((first, pool, reader.slot0()?.0, stock_is_token0));
        }
        pools.push(PoolReport { pool, fee, stock_is_token0, depth });
    }
    if pools.is_empty() {
        notes.push(format!("{sym}: no Uniswap V3 pool against USDG, sellable depth is zero"));
    }

    Ok(StockReport {
        stock,
        symbol: sym,
        listed,
        decimals,
        markets: market_reports,
        borrowed: borrowed.to_string(),
        vault_cap: vault_cap.to_string(),
        collateral: collateral.to_string(),
        collateral_value: collateral_value.map(|v| v.to_string()),
        pools,
        sellable: sellable.iter().map(|v| v.to_string()).collect(),
        price_pool: best.map(|b| b.1),
        price: best.and_then(|(_, _, sp, is0)| {
            lenders::value_at_spot(U256::from(10u8).pow(U256::from(decimals)), sp, is0).map(|v| v.to_string())
        }),
        spot: best.map(|(_, _, sp, is0)| (sp, is0)),
    })
}

fn walk(reader: &RpcPool, stock_is_token0: bool, bps: u64, steps: u32, pool: Address) -> Result<Walk, String> {
    sell_proceeds_bounded(reader, stock_is_token0, bps, steps).map_err(|e| format!("walk {pool} at {bps} bps: {e:?}"))
}

/// Output of the pool's own swap, selling the stock until the same price limit the walk uses.
fn real_swap(rpc: &Rpc, reader: &RpcPool, stock_is_token0: bool, bps: u64) -> Result<U256, String> {
    let (sqrt_price, _) = reader.slot0()?;
    let limit = sqrt_price_limit(sqrt_price, stock_is_token0, bps).ok_or("bad impact")?;
    // A pool already at the price bound (the limit is clamped there) cannot move: the swap would
    // revert with SPL, and the true output is zero.
    if limit == sqrt_price {
        return Ok(U256::ZERO);
    }
    let call = ISwapProbe::probeCall { pool: reader.pool, zeroForOne: stock_is_token0, limit: limit.to() };
    let r = rpc.call_with_code(PROBE, PROBE_CODE.trim(), call)?;
    // The quote token leaves the pool, so its amount is negative.
    let out = if stock_is_token0 { r.amount1 } else { r.amount0 };
    ensure(out <= alloy_primitives::I256::ZERO, || format!("swap paid in quote token: {out}"))?;
    Ok(out.unsigned_abs())
}

/// Total borrow with interest accrued to `now`, as Morpho computes it (see markets::accrue).
fn accrued_borrow(rpc: &Rpc, p: &MarketParams, m: &Market, now: u64) -> (U256, bool) {
    let (a, ok) = markets::accrue(rpc, p, m, now);
    (a.borrow_assets, ok)
}

fn redact(url: &str) -> String {
    // Provider URLs often embed an API key in the path; show only the host.
    match url.split("://").nth(1).and_then(|r| r.split('/').next()) {
        Some(host) => host.to_string(),
        None => "?".into(),
    }
}

/// Formats `v` (in units of 10^-decimals) with `shown` decimals, rounded down.
fn units(v: U256, decimals: u8, shown: u8) -> String {
    let base = U256::from(10u8).pow(U256::from(decimals));
    let whole = v / base;
    let frac = (v % base) / U256::from(10u8).pow(U256::from(decimals.saturating_sub(shown)));
    let mut w = whole.to_string();
    let mut i = w.len();
    while i > 3 {
        i -= 3;
        w.insert(i, ',');
    }
    if shown == 0 {
        w
    } else {
        format!("{w}.{:0>width$}", frac.to_string(), width = shown as usize)
    }
}

fn ratio(num: U256, den: U256) -> String {
    if den.is_zero() {
        return "-".into();
    }
    let x100 = num * U256::from(100u8) / den;
    let x = x100.saturating_to::<u128>();
    format!("{}.{:02}x", x / 100, x % 100)
}

fn print_table(r: &Report, usdg_dec: u8) {
    let u = |s: &str| s.parse::<U256>().unwrap();
    println!();
    println!("Denar equity vault {} on Morpho Blue {}", r.vault, r.morpho_blue);
    println!("Robinhood Chain block {} (unix {}). Amounts in USDG unless noted.", r.block, r.timestamp);
    println!();
    let mut head =
        format!("{:<6} {:>4} {:>12} {:>12} {:>12} {:>12}", "STOCK", "MKTS", "BORROWED", "VAULT CAP", "COLLATERAL", "COLL VALUE");
    for b in &r.impacts_bps {
        head += &format!(" {:>16}", format!("SELLABLE@{}%", *b as f64 / 100.0));
    }
    for b in &r.impacts_bps {
        head += &format!(" {:>11}", format!("COVER@{}%", *b as f64 / 100.0));
    }
    println!("{head}");
    for s in &r.stocks {
        let borrowed = u(&s.borrowed);
        let mut line = format!(
            "{:<6} {:>4} {:>12} {:>12} {:>12} {:>12}",
            s.symbol,
            s.markets.len(),
            units(borrowed, usdg_dec, 2),
            units(u(&s.vault_cap), usdg_dec, 0),
            units(u(&s.collateral), s.decimals, 4),
            s.collateral_value.as_deref().map(|v| units(u(v), usdg_dec, 2)).unwrap_or("?".into()),
        );
        for v in &s.sellable {
            line += &format!(" {:>16}", units(u(v), usdg_dec, 2));
        }
        for v in &s.sellable {
            line += &format!(" {:>11}", ratio(u(v), borrowed));
        }
        println!("{line}");
    }
    println!();
    println!("COLLATERAL is {{stock}}.balanceOf(Morpho Blue) in stock units. COVER is sellable / borrowed.");
    println!();
    println!("Pools (Uniswap V3, stock/USDG):");
    for s in &r.stocks {
        for p in &s.pools {
            let d: Vec<String> = p
                .depth
                .iter()
                .map(|d| {
                    let mark = if !d.complete { "≥" } else if !d.engine_complete { "*" } else { "" };
                    format!("{}bps {}{}", d.impact_bps, mark, units(u(&d.proceeds), usdg_dec, 2))
                })
                .collect();
            println!("  {:<5} {} fee {:>5}  {}", s.symbol, p.pool, p.fee, d.join("  "));
        }
    }
    println!("  (* the onchain engine's {MAX_STEPS}-step bound would report less; ≥ offchain bound hit)");
    if r.verified {
        println!("  Verified: every complete walk above equals the pool's real swap output to the wei.");
    }
    if let Some(l) = &r.lenders {
        print_lenders(r, l, usdg_dec);
    }
    if !r.notes.is_empty() {
        println!();
        println!("Notes:");
        for n in &r.notes {
            println!("  - {n}");
        }
    }
}

fn print_lenders(r: &Report, l: &lenders::LenderReport, usdg_dec: u8) {
    let u = |s: &str| s.parse::<U256>().unwrap();
    println!();
    println!("Stock tokens held by lending contracts, valued at the deepest pool's spot price (USDG):");
    let mut head = format!("{:<30} {:>7}", "PROTOCOL", "HOLDERS");
    for s in &r.stocks {
        head += &format!(" {:>13}", s.symbol);
    }
    head += &format!(" {:>14}", "TOTAL");
    println!("{head}");
    let mut col_totals = vec![U256::ZERO; r.stocks.len()];
    let mut grand = U256::ZERO;
    for p in &l.protocols {
        let mut line = format!("{:<30} {:>7}", p.protocol, p.holders_checked);
        for (i, s) in r.stocks.iter().enumerate() {
            let cell = match p.by_stock.get(&s.symbol) {
                Some((_, Some(v))) => {
                    col_totals[i] += u(v);
                    units(u(v), usdg_dec, 2)
                }
                Some((_, None)) => "?".into(),
                None => "-".into(),
            };
            line += &format!(" {:>13}", cell);
        }
        grand += u(&p.value);
        line += &format!(" {:>14}", units(u(&p.value), usdg_dec, 2));
        println!("{line}");
    }
    let mut line = format!("{:<30} {:>7}", "ALL LENDERS", l.holders_checked);
    for t in &col_totals {
        line += &format!(" {:>13}", units(*t, usdg_dec, 2));
    }
    line += &format!(" {:>14}", units(grand, usdg_dec, 2));
    println!("{line}");
    let mut line = format!("{:<30} {:>7}", "(sellable within first bound)", "");
    for s in &r.stocks {
        line += &format!(" {:>13}", units(u(&s.sellable[0]), usdg_dec, 2));
    }
    println!("{line}");
    println!();
    println!("Prices (spot, deepest Uniswap V3 pool at {} bps):", r.impacts_bps[0]);
    for s in &r.stocks {
        match (&s.price, s.price_pool) {
            (Some(p), Some(pool)) => println!("  {:<5} {:>12} USDG  pool {pool}", s.symbol, units(u(p), usdg_dec, 2)),
            _ => println!("  {:<5} no priced pool", s.symbol),
        }
    }
    if !l.not_covered.is_empty() {
        println!();
        println!("Not covered:");
        for n in &l.not_covered {
            println!("  - {n}");
        }
    }
}
