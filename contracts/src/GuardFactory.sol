// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {ExitlineGuard} from "./ExitlineGuard.sol";
import {IDepthEngine} from "./interfaces/IDepthEngine.sol";
import {IVaultV2Minimal} from "./interfaces/IExternal.sol";

/// @title GuardFactory
/// @notice Deploys an ExitlineGuard for any Morpho Vault V2 vault, all sharing one depth engine.
/// @dev Deploying a guard grants it nothing. It only acts once the vault owner calls
/// `setIsSentinel(guard, true)`. Before doing that, a vault owner should check that the guard's
/// owner is someone they trust, since the guard owner sets the parameters the guard cuts with.
contract GuardFactory {
    IDepthEngine public immutable engine;

    mapping(address guard => bool) public isGuard;
    mapping(address vault => address[]) internal _guardsByVault;

    event GuardCreated(address indexed guard, address indexed vault, address indexed owner, uint64 minGap);

    error ZeroAddress();

    constructor(IDepthEngine _engine) {
        if (address(_engine) == address(0)) revert ZeroAddress();
        engine = _engine;
    }

    /// @notice Deploys a guard for `vault`, owned by `owner`.
    function createGuard(IVaultV2Minimal vault, address owner, uint64 minGap) external returns (ExitlineGuard guard) {
        if (owner == address(0)) revert ZeroAddress();
        guard = new ExitlineGuard(owner, vault, engine, minGap);
        isGuard[address(guard)] = true;
        _guardsByVault[address(vault)].push(address(guard));
        emit GuardCreated(address(guard), address(vault), owner, minGap);
    }

    function guardsByVault(address vault) external view returns (address[] memory) {
        return _guardsByVault[vault];
    }
}
