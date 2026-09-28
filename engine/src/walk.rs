//! Walks a Uniswap V3 pool's initialized ticks the way a swap would, without executing one,
//! and sums what a seller of the stock receives until the price limit is reached.

use alloy_primitives::U256;

use crate::math::{
    amount0_delta_down, amount1_delta_down, compress, next_initialized_within_word, sqrt_price_limit,
    sqrt_ratio_at_tick, word_position, MAX_TICK, MIN_TICK,
};

/// Upper bound on loop iterations. Each iteration costs at most one bitmap read and one tick
/// read. If the bound is hit, the walk stops and reports what it has counted so far, which is
/// less than the true depth. Never more.
pub const MAX_STEPS: u32 = 128;

/// Read access to the pool state the walk needs. Implemented over live contract calls in the
/// Stylus entrypoint and over in-memory fixtures in tests.
pub trait PoolReader {
    type Error;
    fn slot0(&self) -> Result<(U256, i32), Self::Error>;
    fn liquidity(&self) -> Result<u128, Self::Error>;
    fn tick_spacing(&self) -> Result<i32, Self::Error>;
    fn tick_bitmap(&self, word: i16) -> Result<U256, Self::Error>;
    fn liquidity_net(&self, tick: i32) -> Result<i128, Self::Error>;
}

#[derive(Debug, PartialEq, Eq)]
pub enum WalkError<E> {
    Read(E),
    Math,
    BadImpact,
    NegativeLiquidity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Walk {
    /// Quote-token amount a seller receives, rounded down.
    pub proceeds: U256,
    /// False when the step bound stopped the walk before the price limit.
    pub complete: bool,
    pub steps: u32,
}

pub fn sell_proceeds<R: PoolReader>(
    pool: &R,
    stock_is_token0: bool,
    impact_bps: u64,
) -> Result<Walk, WalkError<R::Error>> {
    let (sqrt_price, _) = pool.slot0().map_err(WalkError::Read)?;
    let limit = sqrt_price_limit(sqrt_price, stock_is_token0, impact_bps).ok_or(WalkError::BadImpact)?;
    // Selling token0 pushes the price down (zeroForOne); selling token1 pushes it up.
    walk_to_limit(pool, stock_is_token0, limit)
}

/// Sums the output a swap in direction `zero_for_one` would pay out before reaching `limit`.
pub fn walk_to_limit<R: PoolReader>(
    pool: &R,
    zero_for_one: bool,
    limit: U256,
) -> Result<Walk, WalkError<R::Error>> {
    walk_to_limit_bounded(pool, zero_for_one, limit, MAX_STEPS)
}

/// `sell_proceeds` with a caller-chosen step bound. Offchain tools use it to see past
/// `MAX_STEPS`; the contract always uses `MAX_STEPS`.
pub fn sell_proceeds_bounded<R: PoolReader>(
    pool: &R,
    stock_is_token0: bool,
    impact_bps: u64,
    max_steps: u32,
) -> Result<Walk, WalkError<R::Error>> {
    let (sqrt_price, _) = pool.slot0().map_err(WalkError::Read)?;
    let limit = sqrt_price_limit(sqrt_price, stock_is_token0, impact_bps).ok_or(WalkError::BadImpact)?;
    walk_to_limit_bounded(pool, stock_is_token0, limit, max_steps)
}

/// `walk_to_limit` with a caller-chosen step bound.
pub fn walk_to_limit_bounded<R: PoolReader>(
    pool: &R,
    zero_for_one: bool,
    limit: U256,
    max_steps: u32,
) -> Result<Walk, WalkError<R::Error>> {
    let (mut sqrt_price, mut tick) = pool.slot0().map_err(WalkError::Read)?;
    let mut liquidity = pool.liquidity().map_err(WalkError::Read)?;
    let spacing = pool.tick_spacing().map_err(WalkError::Read)?;
    if spacing <= 0 {
        return Err(WalkError::Math);
    }
    if (zero_for_one && limit > sqrt_price) || (!zero_for_one && limit < sqrt_price) {
        return Err(WalkError::BadImpact);
    }

    let mut proceeds = U256::ZERO;
    let mut cached: Option<(i16, U256)> = None;
    let mut steps = 0u32;

    while sqrt_price != limit {
        if steps == max_steps {
            return Ok(Walk { proceeds, complete: false, steps });
        }
        steps += 1;

        let compressed = compress(tick, spacing);
        let word_pos = if zero_for_one {
            word_position(compressed).0
        } else {
            word_position(compressed + 1).0
        };
        let word = match cached {
            Some((pos, w)) if pos == word_pos => w,
            _ => {
                let w = pool.tick_bitmap(word_pos).map_err(WalkError::Read)?;
                cached = Some((word_pos, w));
                w
            }
        };
        let (mut tick_next, initialized) = next_initialized_within_word(compressed, spacing, zero_for_one, word);
        tick_next = tick_next.clamp(MIN_TICK, MAX_TICK);
        let sqrt_next = sqrt_ratio_at_tick(tick_next).ok_or(WalkError::Math)?;

        let target = if zero_for_one {
            if sqrt_next < limit { limit } else { sqrt_next }
        } else if sqrt_next > limit {
            limit
        } else {
            sqrt_next
        };

        let out = if zero_for_one {
            amount1_delta_down(sqrt_price, target, liquidity)
        } else {
            amount0_delta_down(sqrt_price, target, liquidity)
        }
        .ok_or(WalkError::Math)?;
        proceeds = proceeds.checked_add(out).ok_or(WalkError::Math)?;
        sqrt_price = target;

        if sqrt_price == sqrt_next {
            if initialized {
                let mut net = pool.liquidity_net(tick_next).map_err(WalkError::Read)?;
                if zero_for_one {
                    net = net.checked_neg().ok_or(WalkError::Math)?;
                }
                let next = (liquidity as i128).checked_add(net).ok_or(WalkError::Math)?;
                if next < 0 {
                    return Err(WalkError::NegativeLiquidity);
                }
                liquidity = next as u128;
            }
            tick = if zero_for_one { tick_next - 1 } else { tick_next };
        } else {
            break;
        }
    }

    Ok(Walk { proceeds, complete: true, steps })
}

#[cfg(test)]
pub mod testing {
    use super::*;
    use std::collections::BTreeMap;

    /// An in-memory pool built from positions, with the bitmap and liquidityNet derived the same
    /// way Uniswap V3 derives them on mint.
    #[derive(Default, Clone)]
    pub struct MemPool {
        pub sqrt_price: U256,
        pub tick: i32,
        pub liquidity: u128,
        pub spacing: i32,
        pub bitmap: BTreeMap<i16, U256>,
        pub net: BTreeMap<i32, i128>,
    }

    impl MemPool {
        pub fn new(tick: i32, spacing: i32) -> Self {
            MemPool {
                sqrt_price: sqrt_ratio_at_tick(tick).unwrap(),
                tick,
                spacing,
                ..Default::default()
            }
        }

        fn flip(&mut self, t: i32) {
            let (w, b) = word_position(t / self.spacing);
            let e = self.bitmap.entry(w).or_insert(U256::ZERO);
            *e ^= U256::from(1u8) << (b as usize);
        }

        fn update(&mut self, t: i32, delta: i128) {
            let before = self.net.get(&t).copied();
            let e = self.net.entry(t).or_insert(0);
            *e += delta;
            if before.is_none() {
                self.flip(t);
            }
        }

        pub fn add_position(&mut self, lower: i32, upper: i32, l: u128) {
            assert!(lower % self.spacing == 0 && upper % self.spacing == 0 && lower < upper);
            self.update(lower, l as i128);
            self.update(upper, -(l as i128));
            if lower <= self.tick && self.tick < upper {
                self.liquidity += l;
            }
        }
    }

    impl PoolReader for MemPool {
        type Error = ();
        fn slot0(&self) -> Result<(U256, i32), ()> {
            Ok((self.sqrt_price, self.tick))
        }
        fn liquidity(&self) -> Result<u128, ()> {
            Ok(self.liquidity)
        }
        fn tick_spacing(&self) -> Result<i32, ()> {
            Ok(self.spacing)
        }
        fn tick_bitmap(&self, word: i16) -> Result<U256, ()> {
            Ok(self.bitmap.get(&word).copied().unwrap_or(U256::ZERO))
        }
        fn liquidity_net(&self, tick: i32) -> Result<i128, ()> {
            Ok(self.net.get(&tick).copied().unwrap_or(0))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::MemPool;
    use super::*;
    use crate::math::{amount0_delta_down, amount1_delta_down, sqrt_price_limit};

    const L: u128 = 1_000_000_000_000_000_000_000;

    #[test]
    fn single_wide_range_matches_closed_form_token0() {
        let mut p = MemPool::new(0, 60);
        p.add_position(-6000, 6000, L);
        let w = sell_proceeds(&p, true, 500).unwrap();
        let limit = sqrt_price_limit(p.sqrt_price, true, 500).unwrap();
        assert!(w.complete);
        assert_eq!(w.proceeds, amount1_delta_down(p.sqrt_price, limit, L).unwrap());
    }

    #[test]
    fn single_wide_range_matches_closed_form_token1() {
        let mut p = MemPool::new(0, 60);
        p.add_position(-6000, 6000, L);
        let w = sell_proceeds(&p, false, 500).unwrap();
        let limit = sqrt_price_limit(p.sqrt_price, false, 500).unwrap();
        assert!(w.complete);
        assert_eq!(w.proceeds, amount0_delta_down(p.sqrt_price, limit, L).unwrap());
    }

    #[test]
    fn liquidity_above_the_price_does_not_count_when_selling_token0() {
        let mut p = MemPool::new(0, 60);
        p.add_position(60, 6000, L);
        let w = sell_proceeds(&p, true, 500).unwrap();
        assert_eq!(w.proceeds, U256::ZERO);
    }

    #[test]
    fn narrow_range_is_exhausted_inside_the_bound() {
        // Liquidity only covers the first 60 ticks down; the rest of a 5% move is empty.
        let mut narrow = MemPool::new(0, 60);
        narrow.add_position(-60, 60, L);
        let mut wide = MemPool::new(0, 60);
        wide.add_position(-6000, 6000, L);
        let n = sell_proceeds(&narrow, true, 500).unwrap();
        let w = sell_proceeds(&wide, true, 500).unwrap();
        let expected = amount1_delta_down(narrow.sqrt_price, sqrt_ratio_at_tick(-60).unwrap(), L).unwrap();
        assert_eq!(n.proceeds, expected);
        assert!(n.proceeds < w.proceeds);
    }

    #[test]
    fn stacked_ranges_add_up() {
        let mut p = MemPool::new(0, 10);
        p.add_position(-200, 200, L);
        p.add_position(-100, 50, L);
        // A 1.5% fall ends near tick -151: past the second position's edge, inside the first.
        let w = sell_proceeds(&p, true, 150).unwrap();
        let limit = sqrt_price_limit(p.sqrt_price, true, 150).unwrap();
        let s0 = p.sqrt_price;
        let s100 = sqrt_ratio_at_tick(-100).unwrap();
        let s200 = sqrt_ratio_at_tick(-200).unwrap();
        assert!(limit < s100 && limit > s200);
        let expected = amount1_delta_down(s0, s100, 2 * L).unwrap() + amount1_delta_down(s100, limit, L).unwrap();
        assert!(w.proceeds <= expected);
        assert!(expected - w.proceeds <= U256::from(w.steps));
    }

    #[test]
    fn liquidity_ends_where_positions_end() {
        let mut p = MemPool::new(0, 10);
        p.add_position(-200, 200, L);
        p.add_position(-100, 50, L);
        // A 3% fall runs past tick -200, where all liquidity ends.
        let w = sell_proceeds(&p, true, 300).unwrap();
        let s100 = sqrt_ratio_at_tick(-100).unwrap();
        let s200 = sqrt_ratio_at_tick(-200).unwrap();
        let expected = amount1_delta_down(p.sqrt_price, s100, 2 * L).unwrap() + amount1_delta_down(s100, s200, L).unwrap();
        assert!(w.complete);
        assert!(w.proceeds <= expected);
        assert!(expected - w.proceeds <= U256::from(w.steps));
    }

    #[test]
    fn walks_across_bitmap_words_with_negative_ticks() {
        // Spacing 1 puts neighbouring words 256 ticks apart; start just above a word boundary.
        let mut p = MemPool::new(-250, 1);
        p.add_position(-1000, 10, L);
        let w = sell_proceeds(&p, true, 500).unwrap();
        let limit = sqrt_price_limit(p.sqrt_price, true, 500).unwrap();
        let exact = amount1_delta_down(p.sqrt_price, limit, L).unwrap();
        // Word boundaries split the range into segments, each rounded down: at most 1 wei each.
        assert!(w.proceeds <= exact);
        assert!(exact - w.proceeds <= U256::from(w.steps));
        assert!(w.steps > 1);
    }

    #[test]
    fn empty_pool_returns_zero() {
        let p = MemPool::new(0, 60);
        let w = sell_proceeds(&p, true, 500).unwrap();
        assert_eq!(w.proceeds, U256::ZERO);
        assert!(w.complete);
    }

    #[test]
    fn step_bound_undercounts_rather_than_overcounts() {
        // Many tiny ranges force more crossings than MAX_STEPS allows.
        let mut p = MemPool::new(0, 1);
        for i in 0..300 {
            p.add_position(-(i + 1), -i, L);
        }
        p.add_position(-6000, 6000, L);
        let w = sell_proceeds(&p, true, 500).unwrap();
        assert!(!w.complete);
        let mut full = MemPool::new(0, 1);
        full.add_position(-6000, 6000, L);
        let floor = sell_proceeds(&full, true, 500).unwrap();
        // The partial walk must not exceed the exact answer for the richer pool.
        let limit = sqrt_price_limit(p.sqrt_price, true, 500).unwrap();
        let exact_upper = amount1_delta_down(p.sqrt_price, limit, 2 * L).unwrap();
        assert!(w.proceeds <= exact_upper);
        assert!(floor.complete);
    }

    #[test]
    fn rejects_bad_impact() {
        let p = MemPool::new(0, 60);
        assert_eq!(sell_proceeds(&p, true, 0), Err(WalkError::BadImpact));
        assert_eq!(sell_proceeds(&p, true, 10_000), Err(WalkError::BadImpact));
    }
}
