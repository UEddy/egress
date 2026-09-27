//! Exitline depth engine, an Arbitrum Stylus contract.
//!
//! `sellProceeds(pool, stockIsToken0, maxImpactBps)` returns how much of the quote token a seller
//! of the stock would receive from a Uniswap V3 pool before the pool price falls by
//! `maxImpactBps`. It reads pool state only; it never swaps and holds no funds or storage.
#![cfg_attr(not(any(test, feature = "export-abi")), no_main)]
extern crate alloc;

pub mod math;
pub mod walk;

#[cfg(test)]
mod crosscheck;

use alloc::vec::Vec;

use alloy_primitives::{aliases::I24, Address, U256};
use stylus_sdk::prelude::*;

use crate::walk::{sell_proceeds, PoolReader, WalkError};

sol_interface! {
    interface IUniswapV3PoolState {
        function slot0() external view returns (uint160 sqrtPriceX96, int24 tick, uint16 observationIndex, uint16 observationCardinality, uint16 observationCardinalityNext, uint8 feeProtocol, bool unlocked);
        function liquidity() external view returns (uint128);
        function tickSpacing() external view returns (int24);
        function tickBitmap(int16 word) external view returns (uint256);
        function ticks(int24 tick) external view returns (uint128 liquidityGross, int128 liquidityNet, uint256 feeGrowthOutside0X128, uint256 feeGrowthOutside1X128, int56 tickCumulativeOutside, uint160 secondsPerLiquidityOutsideX128, uint32 secondsOutside, bool initialized);
    }
}

#[storage]
#[entrypoint]
pub struct DepthEngine {}

/// Live pool reads through static calls from the running contract.
struct LivePool<'a, H: Host> {
    host: &'a H,
    pool: IUniswapV3PoolState,
}

fn i24(v: I24) -> i32 {
    v.as_i32()
}

impl<H: Host> PoolReader for LivePool<'_, H> {
    type Error = ();

    fn slot0(&self) -> Result<(U256, i32), ()> {
        let r = self.pool.slot_0(self.host, Call::new()).map_err(|_| ())?;
        Ok((U256::from(r.0), i24(r.1)))
    }

    fn liquidity(&self) -> Result<u128, ()> {
        self.pool.liquidity(self.host, Call::new()).map_err(|_| ())
    }

    fn tick_spacing(&self) -> Result<i32, ()> {
        Ok(i24(self.pool.tick_spacing(self.host, Call::new()).map_err(|_| ())?))
    }

    fn tick_bitmap(&self, word: i16) -> Result<U256, ()> {
        self.pool.tick_bitmap(self.host, Call::new(), word).map_err(|_| ())
    }

    fn liquidity_net(&self, tick: i32) -> Result<i128, ()> {
        let t = I24::try_from(tick).map_err(|_| ())?;
        let r = self.pool.ticks(self.host, Call::new(), t).map_err(|_| ())?;
        Ok(r.1)
    }
}

#[public]
impl DepthEngine {
    /// Quote-token proceeds from selling the stock until the pool price has moved against the
    /// seller by `max_impact_bps`. Reverts if the pool cannot be read or the bound is invalid.
    pub fn sell_proceeds(&self, pool: Address, stock_is_token0: bool, max_impact_bps: U256) -> Result<U256, Vec<u8>> {
        let impact: u64 = max_impact_bps.try_into().map_err(|_| b"bad impact".to_vec())?;
        let reader = LivePool { host: self.vm(), pool: IUniswapV3PoolState::new(pool) };
        match sell_proceeds(&reader, stock_is_token0, impact) {
            Ok(w) => Ok(w.proceeds),
            Err(WalkError::BadImpact) => Err(b"bad impact".to_vec()),
            Err(WalkError::Read(())) => Err(b"pool read failed".to_vec()),
            Err(WalkError::NegativeLiquidity) => Err(b"negative liquidity".to_vec()),
            Err(WalkError::Math) => Err(b"math".to_vec()),
        }
    }
}
