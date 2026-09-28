//! Stock tokens held by lending contracts. `lenders.json` lists each protocol's root addresses and
//! where they came from; the contracts that actually custody collateral (aTokens, loan accounts,
//! per-offer vaults, margin accounts, gearing tokens) are derived onchain at the pinned block.

use std::collections::BTreeMap;

use alloy_primitives::{Address, B256, U256};
use serde::{Deserialize, Serialize};

use crate::rpc::*;

#[derive(Deserialize)]
pub struct Inventory {
    pub protocols: Vec<Protocol>,
    #[serde(default)]
    pub not_covered: Vec<String>,
}

#[derive(Deserialize)]
pub struct Protocol {
    pub name: String,
    pub source: String,
    /// Contracts that hold collateral themselves.
    #[serde(default)]
    holders: Vec<Address>,
    /// Aave V3 pool data providers: each reserve's aToken holds the underlying.
    #[serde(default)]
    aave_v3_data_providers: Vec<Address>,
    /// Ripe VaultBook registries: every registered vault can hold collateral.
    #[serde(default)]
    ripe_vault_books: Vec<Address>,
    /// Gage V2/V3 engines: each loan keeps its collateral in its own account contract.
    #[serde(default)]
    gage_engines: Vec<Address>,
    /// Turret V3 P2P markets: each offer has its own vault.
    #[serde(default)]
    turret_v3_markets: Vec<Address>,
    /// Arcadia account factories: each margin account holds its own collateral.
    #[serde(default)]
    arcadia_factories: Vec<Address>,
    /// TermMax market factories: each market's gearing token holds the collateral.
    #[serde(default)]
    termmax_factories: Vec<TermMaxFactory>,
}

#[derive(Deserialize)]
struct TermMaxFactory {
    address: Address,
    from_block: u64,
}

/// Registry slots probed on a Ripe VaultBook.
const RIPE_MAX_VAULTS: u64 = 32;

pub struct Holder {
    pub protocol: usize,
    pub address: Address,
    pub how: String,
    /// One contract per borrower (loan account, margin account, offer vault). Its address
    /// identifies an individual, so reports leave it out.
    pub per_borrower: bool,
}

pub fn load(path: &str) -> Result<Inventory, String> {
    let raw = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    serde_json::from_str(&raw).map_err(|e| format!("{path}: {e}"))
}

/// Every address that can custody a stock token for the listed protocols, deduplicated.
pub fn discover(rpc: &Rpc, inv: &Inventory, stocks: &[Address], notes: &mut Vec<String>) -> Result<Vec<Holder>, String> {
    let mut out: Vec<Holder> = Vec::new();
    for (pi, p) in inv.protocols.iter().enumerate() {
        let before = out.len();
        let mut add = |address: Address, how: String, per_borrower: bool| {
            if address != Address::ZERO {
                out.push(Holder { protocol: pi, address, how, per_borrower });
            }
        };
        for &a in &p.holders {
            add(a, "listed".into(), false);
        }
        for &dp in &p.aave_v3_data_providers {
            let calls = stocks.iter().map(|&s| (dp, ILenders::getReserveTokensAddressesCall { asset: s })).collect();
            for (r, s) in rpc.call_many(calls)?.into_iter().zip(stocks) {
                // Assets that are not reserves return zero addresses.
                add(r?.aTokenAddress, format!("aToken of {s} at data provider {dp}"), false);
            }
        }
        for &book in &p.ripe_vault_books {
            let calls = (1..=RIPE_MAX_VAULTS).map(|i| (book, ILenders::getAddrCall { regId: U256::from(i) })).collect();
            // Unused slots return the zero address, so any error is a real failure.
            let found: Vec<Address> = rpc.call_many(calls)?.into_iter().collect::<Result<_, _>>()?;
            if found.iter().filter(|a| !a.is_zero()).count() as u64 == RIPE_MAX_VAULTS {
                notes.push(format!("{}: VaultBook {book} has {RIPE_MAX_VAULTS}+ vaults; raise RIPE_MAX_VAULTS", p.name));
            }
            for (i, a) in found.into_iter().enumerate() {
                add(a, format!("vault #{} of VaultBook {book}", i + 1), false);
            }
        }
        for &engine in &p.gage_engines {
            let n = rpc.call(engine, ILenders::loanCountCall {})?.to::<u64>();
            // Loan ids are local to an engine and start at 1.
            let calls = (1..=n).map(|i| (engine, ILenders::getLoanCall { id: U256::from(i) })).collect();
            for (i, r) in rpc.call_many(calls)?.into_iter().enumerate() {
                let loan = r?;
                if stocks.contains(&loan.token) {
                    add(loan.account, format!("account of loan #{} at engine {engine}", i + 1), true);
                }
            }
        }
        for &market in &p.turret_v3_markets {
            // nextOfferId is exclusive; offer ids start at 1.
            let next = rpc.call(market, ILenders::nextOfferIdCall {})?.to::<u64>();
            let calls = (1..next).map(|i| (market, ILenders::vaultsCall { id: U256::from(i) })).collect();
            for (i, r) in rpc.call_many(calls)?.into_iter().enumerate() {
                add(r?, format!("vault of offer #{} at market {market}", i + 1), true);
            }
        }
        for &factory in &p.arcadia_factories {
            let n = rpc.call(factory, ILenders::allAccountsLengthCall {})?.to::<u64>();
            let calls = (0..n).map(|i| (factory, ILenders::allAccountsCall { i: U256::from(i) })).collect();
            for (i, r) in rpc.call_many(calls)?.into_iter().enumerate() {
                add(r?, format!("account #{i} of factory {factory}"), true);
            }
        }
        for f in &p.termmax_factories {
            for &stock in stocks {
                // MarketCreated(address indexed market, address indexed collateral, address indexed
                // debtToken, ...). Collateral sits on the market's gearing token; a stock that is
                // the lent asset sits on the market itself.
                for (topic, collateral_side) in [(2, true), (3, false)] {
                    for log in rpc.logs_by_topic(f.address, topic, stock.into_word(), f.from_block)? {
                        let market = log["topics"][1]
                            .as_str()
                            .and_then(|t| t.parse::<B256>().ok())
                            .map(Address::from_word)
                            .ok_or_else(|| format!("{}: bad MarketCreated log {log}", p.name))?;
                        let t = rpc.call(market, ILenders::tokensCall {})?;
                        let side = if collateral_side { t.collateral } else { t.debt };
                        if side != stock {
                            return Err(format!("{}: market {market} does not use {stock} as logged", p.name));
                        }
                        if collateral_side {
                            add(t.gearingToken, format!("gearing token of market {market}"), false);
                        } else {
                            add(market, format!("market {market} lending the stock"), false);
                        }
                    }
                }
            }
        }
        if out.len() == before {
            notes.push(format!("{}: no candidate holders found", p.name));
        }
    }

    // Keep the first protocol that claims an address, so nothing is counted twice.
    let mut seen = BTreeMap::new();
    out.retain(|h| seen.insert(h.address, h.protocol).is_none());

    let sizes = rpc.code_sizes(&out.iter().map(|h| h.address).collect::<Vec<_>>())?;
    for (h, size) in out.iter().zip(sizes) {
        if size == 0 {
            notes.push(format!("{} holder {} ({}) has no code", inv.protocols[h.protocol].name, h.address, h.how));
        }
    }
    Ok(out)
}

#[derive(Serialize)]
pub struct Holding {
    pub protocol: String,
    /// None for per-borrower contracts, which `how` still locates by index.
    pub holder: Option<Address>,
    pub how: String,
    pub stock: String,
    pub balance: String,
    /// Balance at the pricing pool's spot price, in USDG units, rounded down.
    pub value: Option<String>,
}

#[derive(Serialize)]
pub struct ProtocolTotal {
    pub protocol: String,
    pub source: String,
    pub holders_checked: usize,
    /// Per stock symbol: (balance, value in USDG units).
    pub by_stock: BTreeMap<String, (String, Option<String>)>,
    pub value: String,
}

#[derive(Serialize)]
pub struct LenderReport {
    pub holders_checked: usize,
    pub protocols: Vec<ProtocolTotal>,
    pub holdings: Vec<Holding>,
    pub not_covered: Vec<String>,
}

/// A stock and how to value it: `spot` is (sqrtPriceX96, stock_is_token0) of its pricing pool.
pub struct Priced<'a> {
    pub address: Address,
    pub symbol: &'a str,
    pub spot: Option<(U256, bool)>,
}

/// Holders and their balances, read at the pinned block. Valued later, once pool prices are known.
pub struct Collected {
    stocks: Vec<Address>,
    holders: Vec<Holder>,
    /// balances[stock index * holders.len() + holder index]
    balances: Vec<U256>,
}

pub fn collect(rpc: &Rpc, inv: &Inventory, stocks: &[Address], notes: &mut Vec<String>) -> Result<Collected, String> {
    let holders = discover(rpc, inv, stocks, notes)?;
    let calls: Vec<_> = stocks
        .iter()
        .flat_map(|&s| holders.iter().map(move |h| (s, IERC20::balanceOfCall { who: h.address })))
        .collect();
    let balances = rpc
        .call_many(calls)?
        .into_iter()
        .enumerate()
        .map(|(i, r)| {
            let (s, h) = (stocks[i / holders.len()], holders[i % holders.len()].address);
            r.map_err(|e| format!("{s}.balanceOf({h}): {e}"))
        })
        .collect::<Result<_, _>>()?;
    Ok(Collected { stocks: stocks.to_vec(), holders, balances })
}

pub fn report(c: Collected, inv: &Inventory, stocks: &[Priced]) -> Result<LenderReport, String> {
    let Collected { holders, balances, .. } = &c;
    if c.stocks != stocks.iter().map(|s| s.address).collect::<Vec<_>>() {
        return Err("lender balances were collected for a different stock list".into());
    }

    let mut holdings = Vec::new();
    let mut totals: Vec<BTreeMap<String, (U256, Option<U256>)>> = vec![BTreeMap::new(); inv.protocols.len()];
    for (si, s) in stocks.iter().enumerate() {
        for (hi, h) in holders.iter().enumerate() {
            let bal = balances[si * holders.len() + hi];
            if bal.is_zero() {
                continue;
            }
            let value = s.spot.and_then(|(sp, is0)| value_at_spot(bal, sp, is0));
            let e = totals[h.protocol].entry(s.symbol.to_string()).or_insert((U256::ZERO, Some(U256::ZERO)));
            e.0 += bal;
            e.1 = match (e.1, value) {
                (Some(a), Some(b)) => Some(a + b),
                _ => None,
            };
            holdings.push(Holding {
                protocol: inv.protocols[h.protocol].name.clone(),
                holder: (!h.per_borrower).then_some(h.address),
                how: h.how.clone(),
                stock: s.symbol.to_string(),
                balance: bal.to_string(),
                value: value.map(|v| v.to_string()),
            });
        }
    }

    let protocols = inv
        .protocols
        .iter()
        .enumerate()
        .map(|(pi, p)| {
            let value = totals[pi].values().filter_map(|(_, v)| *v).fold(U256::ZERO, |a, b| a + b);
            ProtocolTotal {
                protocol: p.name.clone(),
                source: p.source.clone(),
                holders_checked: holders.iter().filter(|h| h.protocol == pi).count(),
                by_stock: totals[pi]
                    .iter()
                    .map(|(k, (b, v))| (k.clone(), (b.to_string(), v.map(|v| v.to_string()))))
                    .collect(),
                value: value.to_string(),
            }
        })
        .collect();
    Ok(LenderReport { holders_checked: holders.len(), protocols, holdings, not_covered: inv.not_covered.clone() })
}

/// Quote-token value of `amount` stock at pool spot price, rounded down at each step.
pub fn value_at_spot(amount: U256, sqrt_price: U256, stock_is_token0: bool) -> Option<U256> {
    use exitline_engine::math::{mul_div, q96};
    if sqrt_price.is_zero() {
        return None;
    }
    if stock_is_token0 {
        // price = token1 per token0 = sqrtP^2 / 2^192
        mul_div(mul_div(amount, sqrt_price, q96())?, sqrt_price, q96())
    } else {
        // price = token0 per token1 = 2^192 / sqrtP^2
        mul_div(mul_div(amount, q96(), sqrt_price)?, q96(), sqrt_price)
    }
}

#[cfg(test)]
mod tests {
    use super::value_at_spot;
    use alloy_primitives::U256;
    use exitline_engine::math::{q96, sqrt_ratio_at_tick};

    #[test]
    fn price_one_is_identity_both_ways() {
        let a = U256::from(123_456_789u64);
        assert_eq!(value_at_spot(a, q96(), true), Some(a));
        assert_eq!(value_at_spot(a, q96(), false), Some(a));
    }

    #[test]
    fn values_round_down_and_sides_are_inverse() {
        // tick 1000: token1 per token0 = 1.0001^1000 ~ 1.10517.
        let sp = sqrt_ratio_at_tick(1000).unwrap();
        let one = U256::from(10u64).pow(U256::from(18u8));
        let v0 = value_at_spot(one, sp, true).unwrap();
        let v1 = value_at_spot(one, sp, false).unwrap();
        assert!(v0 > U256::from(1_105_100_000_000_000_000u64) && v0 < U256::from(1_105_200_000_000_000_000u64));
        // Selling token1 for token0 at the same price gives the reciprocal, rounded down.
        let product = v0 * v1 / one;
        assert!(product <= one && one - product < U256::from(10u64));
    }

    #[test]
    fn zero_price_is_unvalued() {
        assert_eq!(value_at_spot(U256::from(1u8), U256::ZERO, false), None);
    }
}
