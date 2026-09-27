// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

/// @notice Computes how much of the quote token a seller would receive by selling the stock
/// into a Uniswap V3 pool until the pool price has fallen by at most `maxImpactBps`.
/// @dev Implemented in Rust as an Arbitrum Stylus contract (engine/). The result must be
/// conservative: fees are deducted and partial ranges are rounded down.
interface IDepthEngine {
    /// @param pool Uniswap V3 pool holding the stock against the quote token.
    /// @param stockIsToken0 True when the stock is token0 in `pool`.
    /// @param maxImpactBps Maximum allowed fall in the pool price, in basis points.
    /// @return proceeds Quote-token amount received, net of pool fees.
    function sellProceeds(address pool, bool stockIsToken0, uint256 maxImpactBps)
        external
        view
        returns (uint256 proceeds);
}
