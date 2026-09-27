//! Replays pool states captured from real Uniswap V3 core contracts (crosscheck/) and compares
//! the engine's walk with the output the real swap paid.

use std::collections::BTreeMap;
use std::str::FromStr;

use alloy_primitives::U256;
use serde::Deserialize;

use crate::math::sqrt_ratio_at_tick;
use crate::walk::{testing::MemPool, walk_to_limit};

#[derive(Deserialize)]
struct Word {
    pos: i16,
    word: String,
}

#[derive(Deserialize)]
struct TickNet {
    tick: i32,
    net: String,
}

#[derive(Deserialize)]
struct Case {
    zero_for_one: bool,
    limit: String,
    amount_out: String,
}

#[derive(Deserialize)]
struct Fixture {
    name: String,
    sqrt_price: String,
    tick: i32,
    liquidity: String,
    spacing: i32,
    words: Vec<Word>,
    ticks: Vec<TickNet>,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct TickSqrt {
    tick: i32,
    sqrt: String,
}

fn u(s: &str) -> U256 {
    U256::from_str(s).unwrap()
}

fn load(name: &str) -> Fixture {
    let path = format!("{}/fixtures/{}.json", env!("CARGO_MANIFEST_DIR"), name);
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("missing {path}: run forge test in crosscheck/"));
    serde_json::from_str(&raw).unwrap()
}

fn pool_from(f: &Fixture) -> MemPool {
    let mut bitmap = BTreeMap::new();
    for w in &f.words {
        bitmap.insert(w.pos, u(&w.word));
    }
    let mut net = BTreeMap::new();
    for t in &f.ticks {
        net.insert(t.tick, i128::from_str(&t.net).unwrap());
    }
    MemPool {
        sqrt_price: u(&f.sqrt_price),
        tick: f.tick,
        liquidity: u128::from_str(&f.liquidity).unwrap(),
        spacing: f.spacing,
        bitmap,
        net,
    }
}

fn check(name: &str) {
    let f = load(name);
    let pool = pool_from(&f);
    assert!(!f.cases.is_empty());
    for c in &f.cases {
        let real = u(&c.amount_out);
        let w = walk_to_limit(&pool, c.zero_for_one, u(&c.limit)).unwrap();
        assert!(w.complete, "{}: walk hit the step bound", f.name);
        // The engine steps through the same segments as the real swap and rounds each the same
        // way, so its answer must equal what the swap actually paid, to the wei.
        assert_eq!(w.proceeds, real, "{} zfo={} limit={}", f.name, c.zero_for_one, c.limit);
        assert!(real > U256::ZERO || w.proceeds.is_zero());
    }
}

#[test]
fn matches_v3_core_wide_single() {
    check("wide_single");
}

#[test]
fn matches_v3_core_stacked_narrow() {
    check("stacked_narrow");
}

#[test]
fn matches_v3_core_sparse_multiword() {
    check("sparse_multiword");
}

#[test]
fn matches_v3_core_positive_price() {
    check("positive_price");
}

#[test]
fn tick_math_matches_v3_core() {
    let path = format!("{}/fixtures/tick_math.json", env!("CARGO_MANIFEST_DIR"));
    let rows: Vec<TickSqrt> = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert!(rows.len() >= 16);
    for r in rows {
        assert_eq!(sqrt_ratio_at_tick(r.tick).unwrap(), u(&r.sqrt), "tick {}", r.tick);
    }
}

