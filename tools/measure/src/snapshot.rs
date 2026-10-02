//! `exitline-measure snapshot`: writes the dashboard's data file.
//!
//! One pinned block, three things the page needs: sellable depth per stock, the chain wide
//! inventory of stock collateral held by lending contracts, and every canonical Morpho Blue market
//! with a stock as collateral together with the Vault V2 vaults supplying it.
//!
//! The file names vaults, markets and pools. It never names an individual: borrowers are a count,
//! and any supplier that is not a registered vault is aggregated without its address.

use std::collections::BTreeMap;

use alloy_primitives::{Address, B256, U256};
use exitline_engine::walk::{PoolReader, MAX_STEPS};
use serde::Serialize;

use crate::rpc::*;
use crate::{lenders, markets, symbol, walk, FEE_TIERS, LISTED, OFFCHAIN_STEPS};

/// The engine deployed on Robinhood Chain mainnet. The page calls this for its live check.
const ENGINE: Address = alloy_primitives::address!("276F4933f06B77912384D64291E062885C33E031");

/// Where one section of a composite snapshot came from.
#[derive(Serialize)]
struct Source {
    section: String,
    file: String,
    block: u64,
    timestamp: u64,
    taken_at_utc: String,
}

#[derive(Serialize)]
pub struct Snapshot {
    chain_id: u64,
    /// The depth block: what the page's live check compares against.
    block: u64,
    timestamp: u64,
    /// `timestamp` as an ISO 8601 UTC string, so the page can show it without a date library.
    taken_at_utc: String,
    usdg: Address,
    usdg_decimals: u8,
    engine: Address,
    morpho_blue: Address,
    impacts_bps: Vec<u64>,
    /// True when the sections were assembled from separate runs at different blocks, in which case
    /// `sources` says which block each one came from and the page labels them individually.
    composite: bool,
    sources: Vec<Source>,
    totals: Totals,
    stocks: Vec<SnapStock>,
    lenders: LenderSummary,
    notes: Vec<String>,
}

#[derive(Serialize)]
struct Totals {
    /// Every listed stock held by every lending contract found, valued in USDG units.
    lent_usdg: String,
    /// Sellable USDG across all stocks at each impact bound, in `impacts_bps` order.
    sellable: Vec<String>,
    holders_checked: usize,
    protocols: usize,
    markets: usize,
}

#[derive(Serialize)]
struct SnapStock {
    symbol: String,
    address: Address,
    decimals: u8,
    /// USDG units per whole stock token, at the deepest pool's spot price.
    price_usdg: Option<String>,
    pools: Vec<SnapPool>,
    /// Sellable USDG per impact bound, in `impacts_bps` order.
    sellable: Vec<String>,
    /// What the onchain engine itself would report, which is lower where its step bound binds.
    engine_sellable: Vec<String>,
    /// Stock units held by every lending contract found, and their value in USDG units.
    lent_units: String,
    lent_usdg: Option<String>,
    /// Only on a composite file: sellable depth as measured in the SAME run as `lent_usdg`, so the
    /// two can be compared at one block. The `sellable` field above is from the newer depth run and
    /// is not comparable with `lent_usdg` directly.
    #[serde(skip_serializing_if = "Option::is_none")]
    sellable_at_lent_block: Option<Vec<String>>,
    markets: Vec<SnapMarket>,
}

#[derive(Serialize)]
struct SnapPool {
    address: Address,
    fee: u32,
    stock_is_token0: bool,
}

#[derive(Serialize, Clone)]
struct SnapMarket {
    id: B256,
    loan_symbol: String,
    loan_decimals: u8,
    lltv_wad: String,
    total_supplied: String,
    total_borrowed: String,
    /// Collateral summed over every position found, in stock units.
    collateral_units: String,
    /// A count, never addresses.
    borrower_count: usize,
    /// Vaults supplying this market, with the allocation each one has to it.
    vaults: Vec<SnapVault>,
    /// Supply by anything that is not a registered vault, aggregated and unnamed.
    other_supplied: String,
}

#[derive(Serialize, Clone)]
struct SnapVault {
    address: Address,
    /// Self-chosen onchain name. Vault V2 curators are addresses only, so there is nothing else.
    name: String,
    /// "vault_v2_adapter" or "vault_v1".
    kind: String,
    /// The vault's supply assets in this market.
    allocation: String,
    curator: Option<Address>,
    owner: Option<Address>,
}

#[derive(Serialize)]
struct LenderSummary {
    holders_checked: usize,
    protocols: Vec<LenderProtocol>,
    not_covered: Vec<String>,
}

#[derive(Serialize)]
struct LenderProtocol {
    name: String,
    source: String,
    holders_checked: usize,
    value_usdg: String,
    /// Stock symbol to value in USDG units.
    by_stock: BTreeMap<String, String>,
}

/// Civil date from a unix timestamp, so the file carries a readable instant without a date crate.
/// Algorithm from Howard Hinnant's civil_from_days.
fn iso_utc(ts: u64) -> String {
    let (days, secs) = ((ts / 86_400) as i64, ts % 86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = era * 400 + yoe + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", secs / 3600, (secs % 3600) / 60, secs % 60)
}

/// Pools for one stock against USDG, and the depth walked in each at every bound.
/// Returns the pools, the full-walk totals, the onchain engine's totals, and the pricing spot.
#[allow(clippy::type_complexity)]
fn depth(
    rpc: &Rpc,
    stock: Address,
    sym: &str,
    usdg: Address,
    impacts: &[u64],
    notes: &mut Vec<String>,
) -> Result<(Vec<SnapPool>, Vec<U256>, Vec<U256>, Option<(U256, bool)>), String> {
    let mut pools = Vec::new();
    let mut sellable = vec![U256::ZERO; impacts.len()];
    let mut engine_sellable = vec![U256::ZERO; impacts.len()];
    let mut best: Option<(U256, U256, bool)> = None;
    for fee in FEE_TIERS {
        let pool = rpc.call(
            crate::UNIV3_FACTORY,
            IUniswapV3Factory::getPoolCall { a: stock, b: usdg, fee: fee.try_into().unwrap() },
        )?;
        if pool == Address::ZERO {
            continue;
        }
        let t0 = rpc.call(pool, IUniswapV3Pool::token0Call {})?;
        let t1 = rpc.call(pool, IUniswapV3Pool::token1Call {})?;
        crate::ensure((t0, t1) == (stock, usdg) || (t0, t1) == (usdg, stock), || {
            format!("pool {pool} tokens {t0}, {t1}")
        })?;
        let stock_is_token0 = t0 == stock;
        let reader = RpcPool::new(rpc, pool);
        reader.prefetch(stock_is_token0, impacts.iter().copied().max().unwrap_or(0))?;
        let mut first = U256::ZERO;
        for (i, &bps) in impacts.iter().enumerate() {
            let engine = walk(&reader, stock_is_token0, bps, MAX_STEPS, pool)?;
            let full = if engine.complete { engine } else { walk(&reader, stock_is_token0, bps, OFFCHAIN_STEPS, pool)? };
            if !engine.complete {
                notes.push(format!(
                    "{sym} pool {pool} at {bps} bps: the onchain engine stops at {MAX_STEPS} steps, the full walk took {}",
                    full.steps
                ));
            }
            if !full.complete {
                notes.push(format!("{sym} pool {pool} at {bps} bps: offchain bound hit, depth is a lower bound"));
            }
            sellable[i] += full.proceeds;
            engine_sellable[i] += engine.proceeds;
            if i == 0 {
                first = full.proceeds;
            }
        }
        if !first.is_zero() && best.is_none_or(|(b, ..)| first > b) {
            best = Some((first, reader.slot0()?.0, stock_is_token0));
        }
        pools.push(SnapPool { address: pool, fee, stock_is_token0 });
    }
    if pools.is_empty() {
        notes.push(format!("{sym}: no Uniswap V3 pool against USDG, so sellable depth is zero"));
    }
    Ok((pools, sellable, engine_sellable, best.map(|(_, sp, is0)| (sp, is0))))
}

/// Markets on canonical Blue with `stock` as collateral, reduced to what the page shows.
///
/// A market with nothing supplied has no suppliers to name and nobody who could have borrowed, so
/// its row is filled from `market(id)` alone. The full scan, which costs three log queries per
/// market and several more once the endpoint splits them by block range, runs only where there is
/// supply to attribute. That is also exactly the set the page lists.
fn stock_markets(
    rpc: &Rpc,
    stock: Address,
    created: &[(B256, Address, u64)],
    now: u64,
) -> Result<Vec<SnapMarket>, String> {
    let mine: Vec<(B256, u64)> =
        created.iter().filter(|(_, c, _)| *c == stock).map(|&(id, _, from)| (id, from)).collect();
    let totals = rpc.call_many(mine.iter().map(|&(id, _)| (markets::BLUE, IMorpho::marketCall { id })).collect())?;

    let mut out = Vec::new();
    for ((id, from), total) in mine.into_iter().zip(totals) {
        let m = total.map_err(|e| format!("market({id}): {e}"))?;
        if m.totalSupplyAssets == 0 {
            let p = rpc.call(markets::BLUE, IMorpho::idToMarketParamsCall { id })?;
            out.push(SnapMarket {
                id,
                loan_symbol: rpc.call(p.loanToken, IERC20::symbolCall {})?,
                loan_decimals: rpc.call(p.loanToken, IERC20::decimalsCall {})?,
                lltv_wad: p.lltv.to_string(),
                total_supplied: "0".into(),
                total_borrowed: m.totalBorrowAssets.to_string(),
                collateral_units: "0".into(),
                borrower_count: 0,
                vaults: Vec::new(),
                other_supplied: "0".into(),
            });
            continue;
        }
        let (row, _) = markets::market_row(rpc, id, from, now, stock)?;
        let mut vaults = Vec::new();
        let mut other = U256::ZERO;
        for s in &row.suppliers {
            match (s.vault, &s.vault_name) {
                (Some(vault), Some(name)) => vaults.push(SnapVault {
                    address: vault,
                    name: name.clone(),
                    kind: s.kind.clone(),
                    allocation: s.assets.clone(),
                    curator: s.curator,
                    owner: s.owner,
                }),
                _ => other += s.assets.parse::<U256>().map_err(|e| format!("supplier assets: {e}"))?,
            }
        }
        out.push(SnapMarket {
            id: row.id,
            loan_symbol: row.loan_symbol.clone(),
            loan_decimals: row.loan_decimals,
            lltv_wad: row.lltv_wad.clone(),
            total_supplied: row.total_supplied.clone(),
            total_borrowed: row.total_borrowed.clone(),
            collateral_units: row.collateral.clone(),
            borrower_count: row.borrowers.len(),
            vaults,
            other_supplied: other.to_string(),
        });
    }
    // Largest market first: that is the one that matters and the one the page leads with.
    out.sort_by_key(|m| std::cmp::Reverse(m.total_supplied.parse::<U256>().unwrap_or(U256::ZERO)));
    Ok(out)
}

pub fn run(
    rpc: &mut Rpc,
    chain_id: u64,
    block: u64,
    timestamp: u64,
    usdg: Address,
    usdg_decimals: u8,
    lenders_path: &str,
    impacts: &[u64],
    out: &str,
) -> Result<(), String> {
    let mut notes = Vec::new();
    let inventory = lenders::load(lenders_path)?;
    let stock_addrs: Vec<Address> = LISTED.iter().map(|(a, _)| *a).collect();

    // One log query for every market Blue has ever created, shared by all six stocks.
    let created = markets::created_markets(rpc)?;
    eprintln!("{} markets on canonical Blue", created.len());

    // Depth is pure eth_call and parallelises fine, one thread per stock. The market scan is log
    // heavy and the public endpoint throttles bursts of eth_getLogs, so all six stocks share one
    // thread and go through it in sequence. The lender scan gets its own. Everything is pinned to
    // the same block, so the split changes timing only, never the numbers.
    let (per_stock, market_results, lender) = std::thread::scope(|sc| {
        let lender = {
            let r = rpc.fork();
            let (inv, addrs) = (&inventory, &stock_addrs);
            sc.spawn(move || {
                let mut n = Vec::new();
                let c = lenders::collect(&r, inv, addrs, &mut n);
                (c, n, r.calls.get())
            })
        };
        let markets_thread = {
            let r = rpc.fork();
            let created = &created;
            sc.spawn(move || {
                let mut out: Vec<(Address, Result<Vec<SnapMarket>, String>)> = Vec::new();
                for &(stock, sym) in LISTED.iter() {
                    eprintln!("markets: scanning {sym}");
                    out.push((stock, stock_markets(&r, stock, created, timestamp)));
                }
                (out, r.calls.get())
            })
        };
        let stocks: Vec<_> = LISTED
            .iter()
            .map(|&(stock, want)| {
                let r = rpc.fork();
                sc.spawn(move || {
                    let mut n = Vec::new();
                    let res = (|| -> Result<_, String> {
                        let sym = symbol(&r, stock)?;
                        crate::ensure(sym == want, || format!("{stock} symbol is {sym:?}, CLAUDE.md lists {want}"))?;
                        let decimals = r.call(stock, IERC20::decimalsCall {})?;
                        let (pools, sellable, engine_sellable, spot) =
                            depth(&r, stock, &sym, usdg, impacts, &mut n)?;
                        Ok((stock, sym, decimals, pools, sellable, engine_sellable, spot))
                    })();
                    (res, n, r.calls.get())
                })
            })
            .collect();
        let stocks: Vec<_> = stocks.into_iter().map(|h| h.join().expect("stock thread panicked")).collect();
        (
            stocks,
            markets_thread.join().expect("markets thread panicked"),
            lender.join().expect("lender thread panicked"),
        )
    });

    let (market_rows, market_calls) = market_results;
    rpc.calls.set(rpc.calls.get() + market_calls);
    let mut by_stock: BTreeMap<Address, Vec<SnapMarket>> = BTreeMap::new();
    for (stock, res) in market_rows {
        by_stock.insert(stock, res?);
    }

    let mut measured = Vec::new();
    for (res, n, calls) in per_stock {
        rpc.calls.set(rpc.calls.get() + calls);
        notes.extend(n);
        let (stock, sym, decimals, pools, sellable, engine_sellable, spot) = res?;
        let markets = by_stock.remove(&stock).unwrap_or_default();
        measured.push((stock, sym, decimals, pools, sellable, engine_sellable, spot, markets));
    }

    let (collected, lnotes, lcalls) = lender;
    rpc.calls.set(rpc.calls.get() + lcalls);
    notes.extend(lnotes);
    let priced: Vec<lenders::Priced> =
        measured.iter().map(|m| lenders::Priced { address: m.0, symbol: &m.1, spot: m.6 }).collect();
    let lender_report = lenders::report(collected?, &inventory, &priced)?;
    eprintln!("lenders: {} holder contracts checked", lender_report.holders_checked);

    // Per stock lender totals, summed from the holdings the scan found.
    let mut lent_units: BTreeMap<String, U256> = BTreeMap::new();
    let mut lent_value: BTreeMap<String, U256> = BTreeMap::new();
    for h in &lender_report.holdings {
        *lent_units.entry(h.stock.clone()).or_default() +=
            h.balance.parse::<U256>().map_err(|e| format!("holding balance: {e}"))?;
        if let Some(v) = &h.value {
            *lent_value.entry(h.stock.clone()).or_default() +=
                v.parse::<U256>().map_err(|e| format!("holding value: {e}"))?;
        }
    }

    let mut stocks = Vec::new();
    let mut total_sellable = vec![U256::ZERO; impacts.len()];
    let mut total_lent = U256::ZERO;
    let mut market_count = 0usize;
    for (address, sym, decimals, pools, sellable, engine_sellable, spot, markets) in measured {
        for (i, v) in sellable.iter().enumerate() {
            total_sellable[i] += *v;
        }
        let value = lent_value.get(&sym).copied();
        total_lent += value.unwrap_or(U256::ZERO);
        market_count += markets.len();
        stocks.push(SnapStock {
            price_usdg: spot.and_then(|(sp, is0)| {
                lenders::value_at_spot(U256::from(10u8).pow(U256::from(decimals)), sp, is0).map(|v| v.to_string())
            }),
            symbol: sym.clone(),
            address,
            decimals,
            pools,
            sellable: sellable.iter().map(|v| v.to_string()).collect(),
            engine_sellable: engine_sellable.iter().map(|v| v.to_string()).collect(),
            lent_units: lent_units.get(&sym).copied().unwrap_or(U256::ZERO).to_string(),
            lent_usdg: value.map(|v| v.to_string()),
            sellable_at_lent_block: None,
            markets,
        });
    }
    // Biggest exposure first.
    stocks.sort_by_key(|s| {
        std::cmp::Reverse(s.lent_usdg.as_deref().unwrap_or("0").parse::<U256>().unwrap_or(U256::ZERO))
    });

    notes.push("Uniswap V4 pools are not counted, so sellable depth is a lower bound.".into());
    notes.push("Sellable totals assume every pool is sold into up to the bound at the same time.".into());
    notes.push("Borrowers are a count only. This file never names an individual.".into());

    let snap = Snapshot {
        chain_id,
        block,
        timestamp,
        taken_at_utc: iso_utc(timestamp),
        usdg,
        usdg_decimals,
        engine: ENGINE,
        morpho_blue: markets::BLUE,
        impacts_bps: impacts.to_vec(),
        composite: false,
        sources: Vec::new(),
        totals: Totals {
            lent_usdg: total_lent.to_string(),
            sellable: total_sellable.iter().map(|v| v.to_string()).collect(),
            holders_checked: lender_report.holders_checked,
            protocols: lender_report.protocols.len(),
            markets: market_count,
        },
        stocks,
        lenders: LenderSummary {
            holders_checked: lender_report.holders_checked,
            protocols: lender_report
                .protocols
                .iter()
                .map(|p| LenderProtocol {
                    name: p.protocol.clone(),
                    source: p.source.clone(),
                    holders_checked: p.holders_checked,
                    value_usdg: p.value.clone(),
                    by_stock: p
                        .by_stock
                        .iter()
                        .filter_map(|(s, (_, v))| v.clone().map(|v| (s.clone(), v)))
                        .collect(),
                })
                .collect(),
            not_covered: lender_report.not_covered.clone(),
        },
        notes,
    };

    let json = serde_json::to_string_pretty(&snap).map_err(|e| e.to_string())?;
    std::fs::write(out, json + "\n").map_err(|e| format!("{out}: {e}"))?;
    eprintln!("{} RPC requests", rpc.calls.get());
    eprintln!("wrote {out}");
    Ok(())
}

/* ------------------------------------------------------------------------------------------- */
/* Composing a snapshot from reports already in tools/measure/results.                          */
/*                                                                                              */
/* A single pinned-block run needs an archive endpoint for state and a wide-range endpoint for  */
/* logs, and on the free tiers available here one or the other throttles before it finishes.    */
/* Composing from reports that already ran, each labelled with its own block, gives the page     */
/* real measured numbers instead of nothing. The live check is what proves currency.             */
/* ------------------------------------------------------------------------------------------- */

fn obj<'a>(v: &'a serde_json::Value, k: &str, file: &str) -> Result<&'a serde_json::Value, String> {
    v.get(k).ok_or_else(|| format!("{file}: missing {k}"))
}

fn as_u64(v: &serde_json::Value, k: &str, file: &str) -> Result<u64, String> {
    obj(v, k, file)?.as_u64().ok_or_else(|| format!("{file}: {k} is not a number"))
}

fn as_str(v: &serde_json::Value, k: &str, file: &str) -> Result<String, String> {
    Ok(obj(v, k, file)?.as_str().ok_or_else(|| format!("{file}: {k} is not a string"))?.to_string())
}

fn as_addr(v: &serde_json::Value, k: &str, file: &str) -> Result<Address, String> {
    as_str(v, k, file)?.parse().map_err(|_| format!("{file}: {k} is not an address"))
}

fn read(path: &str) -> Result<serde_json::Value, String> {
    let raw = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    serde_json::from_str(&raw).map_err(|e| format!("{path}: {e}"))
}

fn base(path: &str) -> String {
    std::path::Path::new(path).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| path.into())
}

/// Builds the dashboard file from existing reports: depth and pools from `depth_file`, the chain
/// wide lender inventory from `lenders_file`, and markets from each of `markets_files` (one per
/// collateral token, as `markets <TOKEN> --json` writes them).
pub fn from_results(
    depth_file: &str,
    lenders_file: &str,
    markets_files: &[String],
    out: &str,
) -> Result<(), String> {
    let depth_doc = read(depth_file)?;
    let lenders_doc = read(lenders_file)?;

    let chain_id = as_u64(&depth_doc, "chain_id", depth_file)?;
    let depth_block = as_u64(&depth_doc, "block", depth_file)?;
    let depth_ts = as_u64(&depth_doc, "timestamp", depth_file)?;
    let usdg = as_addr(&depth_doc, "usdg", depth_file)?;
    let impacts: Vec<u64> = obj(&depth_doc, "impacts_bps", depth_file)?
        .as_array()
        .ok_or_else(|| format!("{depth_file}: impacts_bps is not an array"))?
        .iter()
        .map(|v| v.as_u64().ok_or_else(|| format!("{depth_file}: impacts_bps entry is not a number")))
        .collect::<Result<_, _>>()?;

    let mut sources = vec![
        Source {
            section: "depth".into(),
            file: base(depth_file),
            block: depth_block,
            timestamp: depth_ts,
            taken_at_utc: iso_utc(depth_ts),
        },
        Source {
            section: "lenders".into(),
            file: base(lenders_file),
            block: as_u64(&lenders_doc, "block", lenders_file)?,
            timestamp: as_u64(&lenders_doc, "timestamp", lenders_file)?,
            taken_at_utc: iso_utc(as_u64(&lenders_doc, "timestamp", lenders_file)?),
        },
    ];

    // Lender holdings, summed per stock symbol.
    let lr = obj(&lenders_doc, "lenders", lenders_file)?;
    if lr.is_null() {
        return Err(format!("{lenders_file} has no lender section, so it cannot supply the inventory"));
    }
    // The lender run measured depth too. Keeping it lets the page compare holdings against depth
    // at ONE block, instead of against the newer depth run, which would overstate the gap.
    let mut sellable_same_block: BTreeMap<String, Vec<String>> = BTreeMap::new();
    if let Some(sts) = lenders_doc.get("stocks").and_then(|v| v.as_array()) {
        for st in sts {
            if let (Some(sym), Some(sell)) =
                (st.get("symbol").and_then(|v| v.as_str()), st.get("sellable").and_then(|v| v.as_array()))
            {
                sellable_same_block.insert(
                    sym.to_string(),
                    sell.iter().map(|v| v.as_str().unwrap_or("0").to_string()).collect(),
                );
            }
        }
    }
    let mut lent_units: BTreeMap<String, U256> = BTreeMap::new();
    let mut lent_value: BTreeMap<String, U256> = BTreeMap::new();
    for h in obj(lr, "holdings", lenders_file)?.as_array().ok_or("holdings is not an array")? {
        let sym = as_str(h, "stock", lenders_file)?;
        let bal: U256 = as_str(h, "balance", lenders_file)?.parse().map_err(|_| "bad balance")?;
        *lent_units.entry(sym.clone()).or_default() += bal;
        if let Some(v) = h.get("value").and_then(|v| v.as_str()) {
            *lent_value.entry(sym).or_default() += v.parse::<U256>().map_err(|_| "bad value")?;
        }
    }

    // Markets per collateral symbol, from each markets report.
    let mut markets_by_symbol: BTreeMap<String, Vec<SnapMarket>> = BTreeMap::new();
    for mf in markets_files {
        let doc = read(mf)?;
        let sym = as_str(&doc, "collateral_symbol", mf)?;
        let ts = as_u64(&doc, "timestamp", mf)?;
        sources.push(Source {
            section: format!("markets:{sym}"),
            file: base(mf),
            block: as_u64(&doc, "block", mf)?,
            timestamp: ts,
            taken_at_utc: iso_utc(ts),
        });
        let mut rows = Vec::new();
        for m in obj(&doc, "markets", mf)?.as_array().ok_or("markets is not an array")? {
            let loan_decimals = as_u64(m, "loan_decimals", mf)? as u8;
            let mut vaults = Vec::new();
            let mut other = U256::ZERO;
            for sup in obj(m, "suppliers", mf)?.as_array().ok_or("suppliers is not an array")? {
                let assets: U256 = as_str(sup, "assets", mf)?.parse().map_err(|_| "bad assets")?;
                match (sup.get("vault").and_then(|v| v.as_str()), sup.get("vault_name").and_then(|v| v.as_str())) {
                    (Some(v), Some(name)) => vaults.push(SnapVault {
                        address: v.parse().map_err(|_| "bad vault address")?,
                        name: name.to_string(),
                        kind: as_str(sup, "kind", mf)?,
                        allocation: assets.to_string(),
                        curator: sup.get("curator").and_then(|c| c.as_str()).and_then(|c| c.parse().ok()),
                        owner: sup.get("owner").and_then(|c| c.as_str()).and_then(|c| c.parse().ok()),
                    }),
                    _ => other += assets,
                }
            }
            rows.push(SnapMarket {
                id: as_str(m, "id", mf)?.parse().map_err(|_| "bad market id")?,
                loan_symbol: as_str(m, "loan_symbol", mf)?,
                loan_decimals,
                lltv_wad: as_str(m, "lltv_wad", mf)?,
                total_supplied: as_str(m, "total_supplied", mf)?,
                total_borrowed: as_str(m, "total_borrowed", mf)?,
                collateral_units: as_str(m, "collateral", mf)?,
                borrower_count: m.get("borrowers").and_then(|b| b.as_array()).map(|b| b.len()).unwrap_or(0),
                vaults,
                other_supplied: other.to_string(),
            });
        }
        rows.sort_by_key(|m| std::cmp::Reverse(m.total_supplied.parse::<U256>().unwrap_or(U256::ZERO)));
        markets_by_symbol.insert(sym, rows);
    }

    // Stocks: pools and depth from the depth report, inventory and markets joined on the symbol.
    let mut stocks = Vec::new();
    let mut total_sellable = vec![U256::ZERO; impacts.len()];
    let mut total_lent = U256::ZERO;
    let mut market_count = 0usize;
    for st in obj(&depth_doc, "stocks", depth_file)?.as_array().ok_or("stocks is not an array")? {
        let sym = as_str(st, "symbol", depth_file)?;
        let decimals = as_u64(st, "decimals", depth_file)? as u8;
        let mut pools = Vec::new();
        let mut engine_sellable = vec![U256::ZERO; impacts.len()];
        for p in obj(st, "pools", depth_file)?.as_array().ok_or("pools is not an array")? {
            pools.push(SnapPool {
                address: as_addr(p, "pool", depth_file)?,
                fee: as_u64(p, "fee", depth_file)? as u32,
                stock_is_token0: obj(p, "stock_is_token0", depth_file)?.as_bool().ok_or("bad stock_is_token0")?,
            });
            for (i, d) in obj(p, "depth", depth_file)?.as_array().ok_or("depth is not an array")?.iter().enumerate() {
                if i < engine_sellable.len() {
                    engine_sellable[i] += as_str(d, "engine_proceeds", depth_file)?
                        .parse::<U256>()
                        .map_err(|_| "bad engine_proceeds")?;
                }
            }
        }
        let sellable: Vec<U256> = obj(st, "sellable", depth_file)?
            .as_array()
            .ok_or("sellable is not an array")?
            .iter()
            .map(|v| v.as_str().unwrap_or("0").parse::<U256>().map_err(|_| "bad sellable".to_string()))
            .collect::<Result<_, _>>()?;
        for (i, v) in sellable.iter().enumerate() {
            if i < total_sellable.len() {
                total_sellable[i] += *v;
            }
        }
        let value = lent_value.get(&sym).copied();
        total_lent += value.unwrap_or(U256::ZERO);
        let markets = markets_by_symbol.get(&sym).cloned().unwrap_or_default();
        market_count += markets.len();
        stocks.push(SnapStock {
            symbol: sym.clone(),
            address: as_addr(st, "stock", depth_file)?,
            decimals,
            price_usdg: st.get("price").and_then(|p| p.as_str()).map(|p| p.to_string()),
            pools,
            sellable: sellable.iter().map(|v| v.to_string()).collect(),
            engine_sellable: engine_sellable.iter().map(|v| v.to_string()).collect(),
            lent_units: lent_units.get(&sym).copied().unwrap_or(U256::ZERO).to_string(),
            lent_usdg: value.map(|v| v.to_string()),
            sellable_at_lent_block: sellable_same_block.get(&sym).cloned(),
            markets,
        });
    }
    stocks.sort_by_key(|s| {
        std::cmp::Reverse(s.lent_usdg.as_deref().unwrap_or("0").parse::<U256>().unwrap_or(U256::ZERO))
    });

    let protocols: Vec<LenderProtocol> = obj(lr, "protocols", lenders_file)?
        .as_array()
        .ok_or("protocols is not an array")?
        .iter()
        .map(|p| -> Result<LenderProtocol, String> {
            let by = obj(p, "by_stock", lenders_file)?.as_object().ok_or("by_stock is not an object")?;
            Ok(LenderProtocol {
                name: as_str(p, "protocol", lenders_file)?,
                source: as_str(p, "source", lenders_file)?,
                holders_checked: as_u64(p, "holders_checked", lenders_file)? as usize,
                value_usdg: as_str(p, "value", lenders_file)?,
                by_stock: by
                    .iter()
                    .filter_map(|(k, v)| v.get(1).and_then(|x| x.as_str()).map(|x| (k.clone(), x.to_string())))
                    .collect(),
            })
        })
        .collect::<Result<_, _>>()?;

    let mut notes: Vec<String> = obj(&depth_doc, "notes", depth_file)?
        .as_array()
        .map(|a| a.iter().filter_map(|n| n.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();
    notes.insert(
        0,
        "This file was composed from separate measured runs. Each section carries the block it was \
         measured at, listed under sources. Use Check live for the current depth."
            .into(),
    );
    notes.insert(
        1,
        "Holdings and the depth they are compared against in the headline come from the same run, \
         so that comparison is at one block. The sellable columns are from the newer depth run and \
         are labelled with their own block."
            .into(),
    );
    notes.push("Borrowers are a count only. This file never names an individual.".into());

    let holders_checked = as_u64(lr, "holders_checked", lenders_file)? as usize;
    let snap = Snapshot {
        chain_id,
        block: depth_block,
        timestamp: depth_ts,
        taken_at_utc: iso_utc(depth_ts),
        usdg,
        usdg_decimals: 6,
        engine: ENGINE,
        morpho_blue: markets::BLUE,
        impacts_bps: impacts,
        composite: true,
        sources,
        totals: Totals {
            lent_usdg: total_lent.to_string(),
            sellable: total_sellable.iter().map(|v| v.to_string()).collect(),
            holders_checked,
            protocols: protocols.len(),
            markets: market_count,
        },
        stocks,
        lenders: LenderSummary {
            holders_checked,
            protocols,
            not_covered: obj(lr, "not_covered", lenders_file)?
                .as_array()
                .map(|a| a.iter().filter_map(|n| n.as_str().map(|s| s.to_string())).collect())
                .unwrap_or_default(),
        },
        notes,
    };

    let json = serde_json::to_string_pretty(&snap).map_err(|e| e.to_string())?;
    std::fs::write(out, json + "\n").map_err(|e| format!("{out}: {e}"))?;
    eprintln!("composed {out} from {} sources", snap.sources.len());
    for s in &snap.sources {
        eprintln!("  {:<16} block {:<10} {}", s.section, s.block, s.file);
    }
    Ok(())
}
