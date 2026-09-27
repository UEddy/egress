// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {ERC20} from "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import {IDepthEngine} from "../../src/interfaces/IDepthEngine.sol";

contract MockERC20 is ERC20 {
    uint8 internal immutable _decimals;

    constructor(string memory name, uint8 decimals_) ERC20(name, name) {
        _decimals = decimals_;
    }

    function decimals() public view override returns (uint8) {
        return _decimals;
    }

    function mint(address to, uint256 amount) external {
        _mint(to, amount);
    }
}

/// @dev ERC20 with the ERC-8056 multiplier functions documented for Robinhood Stock Tokens.
contract MockStockToken is MockERC20 {
    uint256 public uiMultiplier = 1e18;
    uint256 public newUIMultiplier = 1e18;
    uint256 public effectiveAt;

    constructor() MockERC20("NVDA", 18) {}

    function scheduleMultiplier(uint256 next, uint256 at) external {
        newUIMultiplier = next;
        effectiveAt = at;
    }
}

contract MockPool {
    address public token0;
    address public token1;

    constructor(address a, address b) {
        (token0, token1) = a < b ? (a, b) : (b, a);
    }
}

contract MockFeed {
    uint256 public updatedAt;
    bool public broken;

    function set(uint256 _updatedAt) external {
        updatedAt = _updatedAt;
    }

    function setBroken(bool b) external {
        broken = b;
    }

    function latestRoundData() external view returns (uint80, int256, uint256, uint256, uint80) {
        require(!broken, "feed down");
        return (1, 100e8, updatedAt, updatedAt, 1);
    }
}

contract MockEngine is IDepthEngine {
    mapping(address => uint256) public proceeds;
    mapping(address => bool) public reverts;

    function set(address pool, uint256 amount) external {
        proceeds[pool] = amount;
    }

    function setReverts(address pool, bool r) external {
        reverts[pool] = r;
    }

    function sellProceeds(address pool, bool, uint256) external view returns (uint256) {
        require(!reverts[pool], "engine revert");
        return proceeds[pool];
    }
}
