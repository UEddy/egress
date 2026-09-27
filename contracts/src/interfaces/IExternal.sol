// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

/// @notice The subset of Morpho Vault V2 that Exitline touches.
/// @dev Signatures match morpho-org/vault-v2 at commit 9ee4dbdc. Verify against the
/// deployed vault with `cast` before configuring a guard on mainnet.
interface IVaultV2Minimal {
    function asset() external view returns (address);
    function curator() external view returns (address);
    function isSentinel(address account) external view returns (bool);
    function absoluteCap(bytes32 id) external view returns (uint256);
    function allocation(bytes32 id) external view returns (uint256);
    function decreaseAbsoluteCap(bytes memory idData, uint256 newAbsoluteCap) external;
}

/// @notice The subset of a Uniswap V3 pool needed to validate a configured pool.
interface IUniswapV3PoolTokens {
    function token0() external view returns (address);
    function token1() external view returns (address);
}

/// @notice ERC-8056 scaled UI amount functions, as documented for Robinhood Stock Tokens.
interface IStockToken {
    function uiMultiplier() external view returns (uint256);
    function newUIMultiplier() external view returns (uint256);
    function effectiveAt() external view returns (uint256);
}

/// @notice Chainlink aggregator read interface.
interface IAggregatorV3 {
    function latestRoundData()
        external
        view
        returns (uint80 roundId, int256 answer, uint256 startedAt, uint256 updatedAt, uint80 answeredInRound);
}
