//! `exitline-measure markets <TOKEN>`: every market on canonical Morpho Blue that takes TOKEN as
//! collateral, with its suppliers, borrowers and the vaults (V1 or V2) that lend into it.
//!
//! Markets come from Blue's CreateMarket logs; suppliers, borrowers and collateral depositors from
//! its Supply, Borrow and SupplyCollateral logs. Every position is then read at the pinned block,
//! so a log only nominates an address and the pinned state decides. Vault types come from Morpho's
//! factory registries where one exists (Vault V2, both V2 adapters). Robinhood Chain has no
//! official MetaMorpho V1 factory (morpho-org/sdks), so V1 vaults are identified by interface.
//! Check: collateral summed over all positions of all markets must equal token.balanceOf(Blue).

use std::collections::BTreeSet;

use alloy_primitives::{address, b256, Address, B256, U256};
use alloy_sol_types::sol;
use serde::Serialize;

use crate::rpc::*;

pub const BLUE: Address = address!("9D53d5E3bd5E8d4Cbfa6DB1ca238AEA02E651010");
const VAULT_V2_FACTORY: Address = address!("0FBad98595b0186dA120E41f77C102beb49f803c");
const MARKET_ADAPTER_FACTORY: Address = address!("79370Ed003CE325C088E530d5e8655c99c2993e1");
const VAULT_V1_ADAPTER_FACTORY: Address = address!("7a91222F3f7B927bB8fb624593Ca86e111C2F85e");

const CREATE_MARKET: B256 = b256!("ac4b2400f169220b0c0afdde7a0b32e775ba727ea1cb30b35f935cdaab8683ac");
const SUPPLY: B256 = b256!("edf8870433c83823eb071d3df1caa8d008f12f6440918c20d75a3602cda30fe0");
const BORROW: B256 = b256!("570954540bed6b1304a87dfe815a5eda4a648f7097a16240dcd85c9b5fd42a43");
/// SupplyCollateral(bytes32 indexed id, address indexed caller, address indexed onBehalf, uint256)
const SUPPLY_COLLATERAL: B256 = b256!("a3b9472a1399e17e123f3c2e6586c23e504184d504de59cdaa2b375e880c6184");
/// CreateMorphoVaultV1Adapter(address indexed parentVault, address indexed morphoVaultV1, address indexed adapter)
const CREATE_V1_ADAPTER: B256 = b256!("2621b71f5d8a3bc363b4162b5e19dcabcba43cfefb95daeda63ba18a54572bc6");

const WAD: u128 = 1_000_000_000_000_000_000;
const VIRTUAL_SHARES: u64 = 1_000_000;
const VIRTUAL_ASSETS: u64 = 1;

sol! {
    struct Position {
        uint256 supplyShares;
        uint128 borrowShares;
        uint128 collateral;
    }

    interface IBlue {
        function position(bytes32 id, address user) external view returns (Position memory);
    }

    interface IVaultLike {
        function MORPHO() external view returns (address);
        function curator() external view returns (address);
        function owner() external view returns (address);
        function name() external view returns (string);
        function parentVault() external view returns (address);
        function isAdapter(address adapter) external view returns (bool);
    }

    interface IRegistries {
        function isVaultV2(address) external view returns (bool);
        function isMorphoMarketV1AdapterV2(address) external view returns (bool);
        function isMorphoVaultV1Adapter(address) external view returns (bool);
    }
}

/// Market totals as Morpho would have them after accruing interest to `now`
/// (MorphoBalancesLib.expectedMarketBalances).
#[derive(Clone, Copy)]
pub struct Accrued {
    pub supply_assets: U256,
    pub supply_shares: U256,
    pub borrow_assets: U256,
    pub borrow_shares: U256,
}

/// Accrues `m` to `now`. The bool is false when the IRM could not be read, in which case the
/// stored totals are returned unchanged.
pub fn accrue(rpc: &Rpc, p: &MarketParams, m: &Market, now: u64) -> (Accrued, bool) {
    let mut a = Accrued {
        supply_assets: U256::from(m.totalSupplyAssets),
        supply_shares: U256::from(m.totalSupplyShares),
        borrow_assets: U256::from(m.totalBorrowAssets),
        borrow_shares: U256::from(m.totalBorrowShares),
    };
    let elapsed = now.saturating_sub(m.lastUpdate as u64);
    if elapsed == 0 || p.irm == Address::ZERO {
        return (a, true);
    }
    let call = IIrm::borrowRateViewCall { marketParams: p.clone(), market: m.clone() };
    let Ok(rate) = rpc.call(p.irm, call) else {
        return (a, false);
    };
    let wad = U256::from(WAD);
    let first = rate * U256::from(elapsed);
    let second = first * first / (U256::from(2u8) * wad);
    let third = second * first / (U256::from(3u8) * wad);
    let interest = a.borrow_assets * (first + second + third) / wad;
    a.borrow_assets += interest;
    a.supply_assets += interest;
    if m.fee != 0 {
        let fee_amount = interest * U256::from(m.fee) / wad;
        let fee_shares = to_shares_down(fee_amount, a.supply_assets - fee_amount, a.supply_shares);
        a.supply_shares += fee_shares;
    }
    (a, true)
}

fn to_shares_down(assets: U256, total_assets: U256, total_shares: U256) -> U256 {
    assets * (total_shares + U256::from(VIRTUAL_SHARES)) / (total_assets + U256::from(VIRTUAL_ASSETS))
}

fn to_assets_down(shares: U256, total_assets: U256, total_shares: U256) -> U256 {
    shares * (total_assets + U256::from(VIRTUAL_ASSETS)) / (total_shares + U256::from(VIRTUAL_SHARES))
}

fn to_assets_up(shares: U256, total_assets: U256, total_shares: U256) -> U256 {
    let num = shares * (total_assets + U256::from(VIRTUAL_ASSETS));
    let den = total_shares + U256::from(VIRTUAL_SHARES);
    num.div_ceil(den)
}

#[derive(Serialize)]
pub struct Supplier {
    /// The supplying contract (vault or adapter). None for any other supplier: an individual.
    pub address: Option<Address>,
    /// Supply assets at the pinned block, rounded down (toAssetsDown on accrued totals).
    pub assets: String,
    /// "vault_v2_adapter", "vault_v1" or "other".
    pub kind: String,
    /// The vault this supply belongs to: the adapter's parent V2 vault, or the V1 vault itself.
    pub vault: Option<Address>,
    pub vault_name: Option<String>,
    pub curator: Option<Address>,
    pub owner: Option<Address>,
    /// For a V1 vault: V2 vaults that hold it through a registered MorphoVaultV1Adapter.
    pub v2_parents: Vec<V2Parent>,
}

#[derive(Serialize)]
pub struct V2Parent {
    pub vault: Address,
    pub name: String,
    pub curator: Address,
    pub owner: Address,
    pub adapter: Address,
    /// Whether the adapter is currently enabled on the parent vault.
    pub enabled: bool,
}

/// A borrower's position. Deliberately without the address: reports never name individuals.
#[derive(Serialize)]
pub struct Borrower {
    /// Debt at the pinned block, rounded up (toAssetsUp), as Morpho measures debt.
    pub debt: String,
    pub collateral: String,
}

#[derive(Serialize)]
pub struct MarketRow {
    pub id: B256,
    pub created_block: u64,
    pub loan_token: Address,
    pub loan_symbol: String,
    pub loan_decimals: u8,
    pub oracle: Address,
    pub irm: Address,
    pub lltv_wad: String,
    pub total_supplied: String,
    pub total_borrowed: String,
    pub accrued: bool,
    /// Sum of all positions' collateral, found through SupplyCollateral and Borrow logs.
    pub collateral: String,
    pub borrowers: Vec<Borrower>,
    pub suppliers: Vec<Supplier>,
}

#[derive(Serialize)]
pub struct MarketsReport {
    pub chain_id: u64,
    pub block: u64,
    pub timestamp: u64,
    pub morpho_blue: Address,
    pub collateral_token: Address,
    pub collateral_symbol: String,
    pub collateral_decimals: u8,
    pub markets_scanned: usize,
    pub markets: Vec<MarketRow>,
    /// token.balanceOf(Blue) and the sum of collateral over all positions found. Equal when
    /// every position was found and Blue holds the token only as collateral.
    pub blue_balance: String,
    pub collateral_found: String,
}

fn topic_addr(log: &serde_json::Value, i: usize) -> Result<Address, String> {
    log["topics"][i]
        .as_str()
        .and_then(|t| t.parse::<B256>().ok())
        .map(Address::from_word)
        .ok_or_else(|| format!("bad log topic {i}: {log}"))
}

fn log_block(log: &serde_json::Value) -> Result<u64, String> {
    let s = log["blockNumber"].as_str().ok_or("log without blockNumber")?;
    u64::from_str_radix(s.trim_start_matches("0x"), 16).map_err(|e| e.to_string())
}

/// Addresses in topic `index` of `event` logs for market `id` since `from`.
fn nominees(rpc: &Rpc, event: B256, id: B256, index: usize, from: u64) -> Result<BTreeSet<Address>, String> {
    rpc.logs(BLUE, &[Some(event), Some(id)], from)?.iter().map(|l| topic_addr(l, index)).collect()
}

pub fn run(rpc: &Rpc, chain_id: u64, block: u64, now: u64, token: Address) -> Result<MarketsReport, String> {
    let symbol = rpc.call(token, IERC20::symbolCall {})?;
    let decimals = rpc.call(token, IERC20::decimalsCall {})?;

    // CreateMarket(bytes32 indexed id, MarketParams marketParams): collateral is the 2nd word.
    let created = rpc.logs(BLUE, &[Some(CREATE_MARKET)], 0)?;
    let mut found = Vec::new();
    for log in &created {
        let data: alloy_primitives::Bytes =
            log["data"].as_str().and_then(|d| d.parse().ok()).ok_or_else(|| format!("bad CreateMarket log {log}"))?;
        if data.len() < 64 {
            return Err(format!("short CreateMarket log {log}"));
        }
        if Address::from_slice(&data[44..64]) == token {
            let id: B256 = log["topics"][1].as_str().and_then(|t| t.parse().ok()).ok_or("bad market id")?;
            found.push((id, log_block(log)?));
        }
    }
    eprintln!("{} markets on Blue, {} with {symbol} collateral", created.len(), found.len());

    let mut markets = Vec::new();
    let mut collateral_found = U256::ZERO;
    for (id, created_block) in found {
        let p = rpc.call(BLUE, IMorpho::idToMarketParamsCall { id })?;
        if p.collateralToken != token {
            return Err(format!("market {id}: idToMarketParams collateral {} != {token}", p.collateralToken));
        }
        let m = rpc.call(BLUE, IMorpho::marketCall { id })?;
        let (acc, accrued) = accrue(rpc, &p, &m, now);

        // Supply(id, caller, onBehalf, ...): onBehalf is topic 3.
        // Borrow(id, caller, onBehalf indexed, receiver indexed, ...): onBehalf is topic 2.
        // SupplyCollateral(id, caller, onBehalf, ...): onBehalf is topic 3.
        let suppliers_n = nominees(rpc, SUPPLY, id, 3, created_block)?;
        let mut holders = nominees(rpc, BORROW, id, 2, created_block)?;
        holders.extend(nominees(rpc, SUPPLY_COLLATERAL, id, 3, created_block)?);
        let people: Vec<Address> = suppliers_n.iter().chain(holders.iter()).copied().collect::<BTreeSet<_>>().into_iter().collect();
        let calls = people.iter().map(|&u| (BLUE, IBlue::positionCall { id, user: u })).collect();
        let positions: Vec<Position> = rpc.call_many(calls)?.into_iter().collect::<Result<_, _>>()?;

        let mut collateral = U256::ZERO;
        let mut borrowers = Vec::new();
        let mut supply_rows = Vec::new();
        for (&u, pos) in people.iter().zip(&positions) {
            collateral += U256::from(pos.collateral);
            if pos.borrowShares != 0 {
                let debt = to_assets_up(U256::from(pos.borrowShares), acc.borrow_assets, acc.borrow_shares);
                borrowers.push(Borrower { debt: debt.to_string(), collateral: pos.collateral.to_string() });
            }
            if !pos.supplyShares.is_zero() {
                let assets = to_assets_down(pos.supplyShares, acc.supply_assets, acc.supply_shares);
                supply_rows.push((u, assets));
            }
        }
        collateral_found += collateral;
        supply_rows.sort_by_key(|r| std::cmp::Reverse(r.1));
        let suppliers = supply_rows
            .into_iter()
            .map(|(u, assets)| classify(rpc, u, assets))
            .collect::<Result<Vec<_>, _>>()?;

        markets.push(MarketRow {
            id,
            created_block,
            loan_symbol: rpc.call(p.loanToken, IERC20::symbolCall {})?,
            loan_decimals: rpc.call(p.loanToken, IERC20::decimalsCall {})?,
            loan_token: p.loanToken,
            oracle: p.oracle,
            irm: p.irm,
            lltv_wad: p.lltv.to_string(),
            total_supplied: acc.supply_assets.to_string(),
            total_borrowed: acc.borrow_assets.to_string(),
            accrued,
            collateral: collateral.to_string(),
            borrowers,
            suppliers,
        });
    }

    let blue_balance = rpc.call(token, IERC20::balanceOfCall { who: BLUE })?;
    Ok(MarketsReport {
        chain_id,
        block,
        timestamp: now,
        morpho_blue: BLUE,
        collateral_token: token,
        collateral_symbol: symbol,
        collateral_decimals: decimals,
        markets_scanned: created.len(),
        markets,
        blue_balance: blue_balance.to_string(),
        collateral_found: collateral_found.to_string(),
    })
}

fn classify(rpc: &Rpc, who: Address, assets: U256) -> Result<Supplier, String> {
    let mut s = Supplier {
        address: None,
        assets: assets.to_string(),
        kind: "other".into(),
        vault: None,
        vault_name: None,
        curator: None,
        owner: None,
        v2_parents: Vec::new(),
    };
    if rpc.call(MARKET_ADAPTER_FACTORY, IRegistries::isMorphoMarketV1AdapterV2Call(who))? {
        let parent = rpc.call(who, IVaultLike::parentVaultCall {})?;
        if !rpc.call(VAULT_V2_FACTORY, IRegistries::isVaultV2Call(parent))? {
            return Err(format!("adapter {who} has parent {parent}, which the Vault V2 factory does not know"));
        }
        s.kind = "vault_v2_adapter".into();
        s.address = Some(who);
        s.vault = Some(parent);
        s.vault_name = Some(rpc.call(parent, IVaultLike::nameCall {})?);
        s.curator = Some(rpc.call(parent, IVaultLike::curatorCall {})?);
        s.owner = Some(rpc.call(parent, IVaultLike::ownerCall {})?);
        return Ok(s);
    }
    // MetaMorpho V1 by interface: points at this Blue and exposes curator() and owner().
    let is_v1 = matches!(rpc.call(who, IVaultLike::MORPHOCall {}), Ok(m) if m == BLUE)
        && rpc.call(who, IVaultLike::curatorCall {}).is_ok();
    if !is_v1 {
        return Ok(s);
    }
    s.kind = "vault_v1".into();
    s.address = Some(who);
    s.vault = Some(who);
    s.vault_name = Some(rpc.call(who, IVaultLike::nameCall {})?);
    s.curator = Some(rpc.call(who, IVaultLike::curatorCall {})?);
    s.owner = Some(rpc.call(who, IVaultLike::ownerCall {})?);
    // CreateMorphoVaultV1Adapter(parentVault, morphoVaultV1, adapter), all indexed.
    for log in rpc.logs(VAULT_V1_ADAPTER_FACTORY, &[Some(CREATE_V1_ADAPTER), None, Some(who.into_word())], 0)? {
        let (parent, adapter) = (topic_addr(&log, 1)?, topic_addr(&log, 3)?);
        if !rpc.call(VAULT_V1_ADAPTER_FACTORY, IRegistries::isMorphoVaultV1AdapterCall(adapter))?
            || !rpc.call(VAULT_V2_FACTORY, IRegistries::isVaultV2Call(parent))?
        {
            return Err(format!("V1 adapter log for {who} names unregistered adapter {adapter} or vault {parent}"));
        }
        s.v2_parents.push(V2Parent {
            vault: parent,
            name: rpc.call(parent, IVaultLike::nameCall {})?,
            curator: rpc.call(parent, IVaultLike::curatorCall {})?,
            owner: rpc.call(parent, IVaultLike::ownerCall {})?,
            adapter,
            enabled: rpc.call(parent, IVaultLike::isAdapterCall { adapter })?,
        });
    }
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn share_math_matches_morpho_rounding() {
        let (ta, ts) = (U256::from(1_000_000u64), U256::from(1_000_000_000_000u64));
        // 1 share of 1e12 on 1e6 assets: down rounds to 0, up rounds to 1.
        assert_eq!(to_assets_down(U256::from(1u8), ta, ts), U256::ZERO);
        assert_eq!(to_assets_up(U256::from(1u8), ta, ts), U256::from(1u8));
        // Round trip never creates assets.
        let s = to_shares_down(U256::from(12_345u64), ta, ts);
        assert!(to_assets_down(s, ta, ts) <= U256::from(12_345u64));
    }
}
