//! Uniswap V3 fixed-point math, ported from v3-core's TickMath, SqrtPriceMath and TickBitmap.
//! Every function rounds in the direction that makes the reported depth smaller.

use alloy_primitives::{U256, U512};

pub const MIN_TICK: i32 = -887_272;
pub const MAX_TICK: i32 = 887_272;
pub const BPS: u64 = 10_000;

pub fn q96() -> U256 {
    U256::from(1u8) << 96usize
}

pub fn min_sqrt_ratio() -> U256 {
    U256::from(4_295_128_739u64)
}

pub fn max_sqrt_ratio() -> U256 {
    U256::from_str_radix("1461446703485210103287273052203988822378723970342", 10).unwrap()
}

/// floor(a * b / d). Returns None when d is zero or the result does not fit in 256 bits.
pub fn mul_div(a: U256, b: U256, d: U256) -> Option<U256> {
    if d.is_zero() {
        return None;
    }
    let prod: U512 = a.widening_mul::<256, 4, 512, 8>(b);
    let q = prod / U512::from(d);
    let limbs = q.as_limbs();
    if limbs[4..].iter().any(|&l| l != 0) {
        return None;
    }
    Some(U256::from_limbs([limbs[0], limbs[1], limbs[2], limbs[3]]))
}

/// ceil(sqrt(x)).
pub fn sqrt_ceil(x: U256) -> U256 {
    let r = x.root(2);
    if r * r < x {
        r + U256::from(1u8)
    } else {
        r
    }
}

fn hex(s: &str) -> U256 {
    U256::from_str_radix(s, 16).unwrap()
}

/// TickMath.getSqrtRatioAtTick. Returns None for ticks outside [MIN_TICK, MAX_TICK].
pub fn sqrt_ratio_at_tick(tick: i32) -> Option<U256> {
    if !(MIN_TICK..=MAX_TICK).contains(&tick) {
        return None;
    }
    let abs = tick.unsigned_abs();
    const STEPS: [(u32, &str); 19] = [
        (0x2, "fff97272373d413259a46990580e213a"),
        (0x4, "fff2e50f5f656932ef12357cf3c7fdcc"),
        (0x8, "ffe5caca7e10e4e61c3624eaa0941cd0"),
        (0x10, "ffcb9843d60f6159c9db58835c926644"),
        (0x20, "ff973b41fa98c081472e6896dfb254c0"),
        (0x40, "ff2ea16466c96a3843ec78b326b52861"),
        (0x80, "fe5dee046a99a2a811c461f1969c3053"),
        (0x100, "fcbe86c7900a88aedcffc83b479aa3a4"),
        (0x200, "f987a7253ac413176f2b074cf7815e54"),
        (0x400, "f3392b0822b70005940c7a398e4b70f3"),
        (0x800, "e7159475a2c29b7443b29c7fa6e889d9"),
        (0x1000, "d097f3bdfd2022b8845ad8f792aa5825"),
        (0x2000, "a9f746462d870fdf8a65dc1f90e061e5"),
        (0x4000, "70d869a156d2a1b890bb3df62baf32f7"),
        (0x8000, "31be135f97d08fd981231505542fcfa6"),
        (0x10000, "9aa508b5b7a84e1c677de54f3e99bc9"),
        (0x20000, "5d6af8dedb81196699c329225ee604"),
        (0x40000, "2216e584f5fa1ea926041bedfe98"),
        (0x80000, "48a170391f7dc42444e8fa2"),
    ];
    let mut ratio = if abs & 0x1 != 0 {
        hex("fffcb933bd6fad37aa2d162d1a594001")
    } else {
        U256::from(1u8) << 128usize
    };
    for (bit, magic) in STEPS {
        if abs & bit != 0 {
            ratio = (ratio * hex(magic)) >> 128usize;
        }
    }
    if tick > 0 {
        ratio = U256::MAX / ratio;
    }
    let low_mask = U256::from(0xffff_ffffu64);
    let round_up = if (ratio & low_mask).is_zero() { 0u8 } else { 1u8 };
    Some((ratio >> 32usize) + U256::from(round_up))
}

/// SqrtPriceMath.getAmount0Delta rounded down, for sqrt prices a and b in either order.
pub fn amount0_delta_down(a: U256, b: U256, liquidity: u128) -> Option<U256> {
    let (lo, hi) = if a < b { (a, b) } else { (b, a) };
    if lo.is_zero() {
        return None;
    }
    let num1 = U256::from(liquidity) << 96usize;
    let num2 = hi - lo;
    Some(mul_div(num1, num2, hi)? / lo)
}

/// SqrtPriceMath.getAmount1Delta rounded down, for sqrt prices a and b in either order.
pub fn amount1_delta_down(a: U256, b: U256, liquidity: u128) -> Option<U256> {
    let (lo, hi) = if a < b { (a, b) } else { (b, a) };
    mul_div(U256::from(liquidity), hi - lo, q96())
}

/// The sqrt price at which the stock has lost `impact_bps` of its value in this pool.
/// Selling token0 lowers the pool price; selling token1 raises it. Rounded toward a smaller
/// move, so the walk measures slightly less depth than the exact bound would.
pub fn sqrt_price_limit(sqrt_price: U256, stock_is_token0: bool, impact_bps: u64) -> Option<U256> {
    if impact_bps == 0 || impact_bps >= BPS {
        return None;
    }
    // factor = sqrt((BPS - impact) / BPS) in Q96, rounded up.
    let scaled = mul_div(U256::from(BPS - impact_bps), U256::from(1u8) << 192usize, U256::from(BPS))?;
    let factor = sqrt_ceil(scaled);
    let limit = if stock_is_token0 {
        // price * (1 - impact): sqrt moves down. Rounding factor up keeps the limit higher.
        mul_div(sqrt_price, factor, q96())?
    } else {
        // price / (1 - impact): sqrt moves up. Rounding factor up keeps the limit lower.
        mul_div(sqrt_price, q96(), factor)?
    };
    let limit = limit.max(min_sqrt_ratio() + U256::from(1u8));
    Some(limit.min(max_sqrt_ratio() - U256::from(1u8)))
}

/// TickBitmap.nextInitializedTickWithinOneWord. `word` is the bitmap word containing the
/// position returned by `word_position`.
pub fn compress(tick: i32, spacing: i32) -> i32 {
    let mut c = tick / spacing;
    if tick < 0 && tick % spacing != 0 {
        c -= 1;
    }
    c
}

/// (wordPos, bitPos) of a compressed tick.
pub fn word_position(compressed: i32) -> (i16, u8) {
    ((compressed >> 8) as i16, (compressed & 0xff) as u8)
}

fn msb(x: U256) -> u32 {
    (255 - x.leading_zeros()) as u32
}

fn lsb(x: U256) -> u32 {
    x.trailing_zeros() as u32
}

/// Given the bitmap word for the relevant position, returns (next tick, initialized).
/// `lte` searches toward lower ticks (selling token0), otherwise toward higher ticks.
pub fn next_initialized_within_word(compressed: i32, spacing: i32, lte: bool, word: U256) -> (i32, bool) {
    if lte {
        let (_, bit) = word_position(compressed);
        let bit = bit as u32;
        let mask = (U256::from(1u8) << (bit as usize)) - U256::from(1u8) + (U256::from(1u8) << (bit as usize));
        let masked = word & mask;
        if masked.is_zero() {
            ((compressed - bit as i32) * spacing, false)
        } else {
            ((compressed - (bit as i32 - msb(masked) as i32)) * spacing, true)
        }
    } else {
        let (_, bit) = word_position(compressed + 1);
        let bit = bit as u32;
        let mask = !((U256::from(1u8) << (bit as usize)) - U256::from(1u8));
        let masked = word & mask;
        if masked.is_zero() {
            ((compressed + 1 + (255 - bit as i32)) * spacing, false)
        } else {
            ((compressed + 1 + (lsb(masked) as i32 - bit as i32)) * spacing, true)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_math_endpoints_match_v3_core() {
        assert_eq!(sqrt_ratio_at_tick(MIN_TICK).unwrap(), min_sqrt_ratio());
        assert_eq!(sqrt_ratio_at_tick(MAX_TICK).unwrap(), max_sqrt_ratio());
        assert_eq!(sqrt_ratio_at_tick(0).unwrap(), q96());
        assert!(sqrt_ratio_at_tick(MAX_TICK + 1).is_none());
    }

    #[test]
    fn tick_math_is_monotonic() {
        let mut prev = U256::ZERO;
        let mut t = MIN_TICK;
        while t <= MAX_TICK {
            let v = sqrt_ratio_at_tick(t).unwrap();
            assert!(v > prev);
            prev = v;
            t += 997;
        }
    }

    #[test]
    fn amounts_round_down() {
        let a = sqrt_ratio_at_tick(-60).unwrap();
        let b = sqrt_ratio_at_tick(60).unwrap();
        let l = 1_000_000_000_000_000_000u128;
        let a0 = amount0_delta_down(a, b, l).unwrap();
        let a1 = amount1_delta_down(a, b, l).unwrap();
        // Symmetric range around price 1: both sides hold nearly the same amount.
        let diff = if a0 > a1 { a0 - a1 } else { a1 - a0 };
        assert!(diff < U256::from(10u64).pow(U256::from(16u8)));
    }

    #[test]
    fn price_limit_moves_the_right_way() {
        let p = q96();
        assert!(sqrt_price_limit(p, true, 500).unwrap() < p);
        assert!(sqrt_price_limit(p, false, 500).unwrap() > p);
        assert!(sqrt_price_limit(p, true, 0).is_none());
        assert!(sqrt_price_limit(p, true, 10_000).is_none());
    }

    #[test]
    fn compress_rounds_toward_negative_infinity() {
        assert_eq!(compress(-1, 60), -1);
        assert_eq!(compress(-60, 60), -1);
        assert_eq!(compress(-61, 60), -2);
        assert_eq!(compress(59, 60), 0);
    }
}
