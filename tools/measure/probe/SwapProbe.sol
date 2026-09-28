// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

interface IUniswapV3PoolSwap {
    function swap(address recipient, bool zeroForOne, int256 amountSpecified, uint160 sqrtPriceLimitX96, bytes calldata data)
        external
        returns (int256 amount0, int256 amount1);
}

/// Never deployed. exitline-measure --verify places this runtime code at an unused address with an
/// eth_call state override, then runs the pool's real swap to a price limit. The swap callback
/// reverts with the amounts, so nothing is paid and no state survives the call.
contract SwapProbe {
    function probe(address pool, bool zeroForOne, uint160 limit) external returns (int256 amount0, int256 amount1) {
        try IUniswapV3PoolSwap(pool).swap(address(this), zeroForOne, type(int128).max, limit, "") {
            revert("swap did not revert");
        } catch (bytes memory r) {
            if (r.length != 64) {
                assembly {
                    revert(add(r, 32), mload(r))
                }
            }
            return abi.decode(r, (int256, int256));
        }
    }

    function uniswapV3SwapCallback(int256 amount0, int256 amount1, bytes calldata) external pure {
        assembly {
            mstore(0, amount0)
            mstore(32, amount1)
            revert(0, 64)
        }
    }
}
