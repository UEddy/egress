//! `egress-measure verify-engine <results.json>`: checks the DEPLOYED engine against a results file.
//!
//! The file already records what the offchain walk and the pool's own `swap()` produced. This asks
//! the engine that is actually deployed on mainnet the same questions, at the same pinned block,
//! and writes the tally back into the file as `engine_verification`. Anything quoting a "N of N"
//! figure should read it from there rather than recomputing it from the pool count, so the claim is
//! a record of a run rather than an assumption.

use alloy_primitives::{Address, U256};
use serde_json::{json, Value};

use crate::rpc::{IDepthEngine, Rpc};

pub fn run(rpc: &mut Rpc, engine: Address, path: &str) -> Result<(), String> {
    let raw = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let mut doc: Value = serde_json::from_str(&raw).map_err(|e| format!("{path}: {e}"))?;

    let block = doc["block"].as_u64().ok_or_else(|| format!("{path}: no block"))?;
    let (pinned, _) = rpc.pin_block(Some(block))?;
    if pinned != block {
        return Err(format!("pinned {pinned}, wanted {block}"));
    }

    let stocks = doc["stocks"].as_array().ok_or("stocks is not an array")?.clone();
    let mut cases = 0usize;
    let mut matched = 0usize;
    let mut mismatches: Vec<Value> = Vec::new();

    for s in &stocks {
        let symbol = s["symbol"].as_str().unwrap_or("?");
        for p in s["pools"].as_array().ok_or("pools is not an array")? {
            let pool: Address = p["pool"].as_str().ok_or("pool")?.parse().map_err(|_| "bad pool")?;
            let is0 = p["stock_is_token0"].as_bool().ok_or("stock_is_token0")?;
            for d in p["depth"].as_array().ok_or("depth is not an array")? {
                let bps = d["impact_bps"].as_u64().ok_or("impact_bps")?;
                let want = d["engine_proceeds"].as_str().ok_or("engine_proceeds")?;
                let got = rpc.call(
                    engine,
                    IDepthEngine::sellProceedsCall { pool, stockIsToken0: is0, maxImpactBps: U256::from(bps) },
                )?;
                cases += 1;
                if got.to_string() == want {
                    matched += 1;
                } else {
                    mismatches.push(json!({
                        "stock": symbol, "pool": pool.to_string(), "impact_bps": bps,
                        "file": want, "onchain": got.to_string(),
                    }));
                }
            }
        }
    }

    eprintln!("{matched} of {cases} readings match the deployed engine, {} mismatched", mismatches.len());
    doc["engine_verification"] = json!({
        "engine": engine.to_string(),
        "block": block,
        "cases": cases,
        "matched": matched,
        "mismatched": mismatches.len(),
        "mismatch_detail": mismatches,
        "note": "Each case is one pool at one impact bound. `matched` means the deployed engine \
                 returned exactly what this file records for that case, to the wei.",
    });

    let out = serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())?;
    std::fs::write(path, out + "\n").map_err(|e| format!("{path}: {e}"))?;
    eprintln!("wrote {path}");
    Ok(())
}
