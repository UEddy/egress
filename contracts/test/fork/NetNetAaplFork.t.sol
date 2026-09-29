// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {Test} from "forge-std/Test.sol";

import {IVaultV2} from "vault-v2/interfaces/IVaultV2.sol";
import {ErrorsLib} from "vault-v2/libraries/ErrorsLib.sol";

import {ExitlineGuard} from "../../src/ExitlineGuard.sol";
import {GuardFactory} from "../../src/GuardFactory.sol";
import {IVaultV2Minimal} from "../../src/interfaces/IExternal.sol";
import {MockFeed, MockEngine} from "../mocks/Mocks.sol";

/// @notice Simulation on a local fork of Robinhood Chain mainnet: an ExitlineGuard on the real
/// NetNet Credit Vault V2, cutting its AAPL collateral cap. Nothing is broadcast; the owner and
/// curator are impersonated only inside the fork.
///
/// The depth engine is a mock that returns, per AAPL/USDG pool, what tools/measure reported at the
/// fork block. Run through script/fork-netnet-aapl.sh, which measures and then forks the same
/// block. Without FORK_BLOCK set, the test is skipped, so plain `forge test` needs no network.
///
/// Env: FORK_BLOCK, AAPL_POOLS and AAPL_DEPTHS (comma lists, depth in USDG units at 5% impact),
/// optional FORK_RPC_URL (defaults to the public endpoint, which keeps only ~10 min of state).
contract NetNetAaplForkTest is Test {
    string constant PUBLIC_RPC = "https://rpc.mainnet.chain.robinhood.com";
    IVaultV2 constant VAULT = IVaultV2(0x99347d5F70D3838763f6Bddcf80304C8aa953B57); // NetNet Credit
    address constant AAPL = 0xaF3D76f1834A1d425780943C99Ea8A608f8a93f9;
    address constant USDG = 0x5fc5360D0400a0Fd4f2af552ADD042D716F1d168;

    uint64 constant GAP = 10 minutes;
    uint256 constant ENGINE_GAS = 1_000_000;
    uint16 constant COVERAGE_BPS = 5_000; // lend against half of sellable depth
    uint16 constant IMPACT_BPS = 500; // depth measured to a 5% price fall (matches AAPL_DEPTHS)

    address guardOwner = makeAddr("guardOwner");
    address keeper = makeAddr("keeper");

    bool active;
    uint256 forkBlock;
    address[] pools;
    uint256[] depths;
    MockEngine engine;
    MockFeed feed;
    ExitlineGuard guard;

    function setUp() public {
        forkBlock = vm.envOr("FORK_BLOCK", uint256(0));
        if (forkBlock == 0) return;
        active = true;

        vm.createSelectFork(vm.envOr("FORK_RPC_URL", PUBLIC_RPC), forkBlock);
        pools = vm.envAddress("AAPL_POOLS", ",");
        depths = vm.envUint("AAPL_DEPTHS", ",");
        require(pools.length == depths.length && pools.length > 0, "AAPL_POOLS / AAPL_DEPTHS mismatch");
        require(VAULT.asset() == USDG, "vault asset is not USDG");

        engine = new MockEngine();
        for (uint256 i; i < pools.length; ++i) {
            engine.set(pools[i], depths[i]);
        }
        feed = new MockFeed();
        feed.set(block.timestamp);

        guard = new GuardFactory(engine).createGuard(IVaultV2Minimal(address(VAULT)), guardOwner, GAP, ENGINE_GAS);

        // Fork only: the real owner makes the guard a sentinel.
        vm.prank(VAULT.owner());
        VAULT.setIsSentinel(address(guard), true);

        ExitlineGuard.Config memory c;
        c.coverageBps = COVERAGE_BPS;
        c.maxImpactBps = IMPACT_BPS;
        c.closedHaircutBps = 5_000;
        c.maxOracleAge = 1 hours;
        c.corporateActionWindow = 2 days;
        c.feed = address(feed);
        vm.startPrank(guardOwner);
        guard.setKeeper(keeper, true);
        guard.configure(AAPL, c, pools);
        vm.stopPrank();
    }

    function _cap() internal view returns (uint256) {
        return VAULT.absoluteCap(guard.capId(AAPL));
    }

    function _record() internal returns (uint256 target) {
        feed.set(block.timestamp);
        vm.prank(keeper);
        target = guard.record(AAPL);
        vm.warp(block.timestamp + GAP);
    }

    function _median(ExitlineGuard.Reading[] memory r) internal pure returns (uint256) {
        uint256[] memory v = new uint256[](r.length);
        for (uint256 i; i < r.length; ++i) {
            v[i] = r[i].target;
        }
        for (uint256 i = 1; i < v.length; ++i) {
            uint256 x = v[i];
            uint256 j = i;
            while (j > 0 && v[j - 1] > x) {
                v[j] = v[j - 1];
                --j;
            }
            v[j] = x;
        }
        return v[v.length / 2];
    }

    function test_fork_guardCutsNetNetAaplCapAndCannotRaiseIt() public {
        if (!active) vm.skip(true);

        bytes memory idData = guard.idData(AAPL);
        uint256 oldCap = _cap();
        uint256 depth;
        for (uint256 i; i < depths.length; ++i) {
            depth += depths[i];
        }
        uint256 expected = depth * COVERAGE_BPS / 10_000;

        emit log_named_uint("fork block", forkBlock);
        emit log_named_decimal_uint("AAPL sellable within 5% (USDG)", depth, 6);
        emit log_named_decimal_uint("allocation to AAPL (USDG)", VAULT.allocation(guard.capId(AAPL)), 6);
        assertGt(oldCap, expected, "cap already at or below the target: nothing to cut");

        // Four readings: no cut before the window is full.
        for (uint256 i; i < guard.WINDOW() - 1; ++i) {
            assertEq(_record(), expected);
            assertEq(_cap(), oldCap, "cut before the window was full");
        }
        // Fifth reading: cut to the median.
        assertEq(_record(), expected);
        uint256 newCap = _cap();
        assertEq(newCap, _median(guard.readings(AAPL)), "cap is not the median reading");
        assertEq(newCap, expected);
        emit log_named_decimal_uint("old AAPL cap (USDG)", oldCap, 6);
        emit log_named_decimal_uint("new AAPL cap (USDG)", newCap, 6);

        // The guard can never raise the cap.
        // 1. Readings far above the cap leave it where it is, even once they fill the window.
        for (uint256 i; i < pools.length; ++i) {
            engine.set(pools[i], depths[i] * 100);
        }
        for (uint256 i; i < guard.WINDOW(); ++i) {
            _record();
        }
        assertEq(_cap(), newCap, "guard raised the cap");
        // 2. As a sentinel it cannot submit a raise, and cannot raise without one.
        bytes memory raise = abi.encodeCall(IVaultV2.increaseAbsoluteCap, (idData, oldCap));
        vm.prank(address(guard));
        vm.expectRevert(ErrorsLib.Unauthorized.selector);
        VAULT.submit(raise);
        vm.prank(address(guard));
        vm.expectRevert(ErrorsLib.DataNotTimelocked.selector);
        VAULT.increaseAbsoluteCap(idData, oldCap);
        assertEq(_cap(), newCap);

        // The curator can still raise it, but only through the timelock.
        uint256 timelock = VAULT.timelock(IVaultV2.increaseAbsoluteCap.selector);
        emit log_named_uint("curator timelock for increaseAbsoluteCap (s)", timelock);
        vm.prank(VAULT.curator());
        VAULT.submit(raise);
        if (timelock > 0) {
            vm.expectRevert(ErrorsLib.TimelockNotExpired.selector);
            VAULT.increaseAbsoluteCap(idData, oldCap);
            vm.warp(block.timestamp + timelock);
        }
        vm.prank(VAULT.curator());
        VAULT.increaseAbsoluteCap(idData, oldCap);
        assertEq(_cap(), oldCap, "curator could not restore the cap");
        emit log_named_decimal_uint("cap after curator raise (USDG)", _cap(), 6);
    }
}
