// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {Test, Vm} from "forge-std/Test.sol";

import {VaultV2Factory} from "vault-v2/VaultV2Factory.sol";
import {IVaultV2} from "vault-v2/interfaces/IVaultV2.sol";
import {ErrorsLib} from "vault-v2/libraries/ErrorsLib.sol";

import {Ownable} from "@openzeppelin/contracts/access/Ownable.sol";

import {ExitlineGuard} from "../src/ExitlineGuard.sol";
import {GuardFactory} from "../src/GuardFactory.sol";
import {IVaultV2Minimal} from "../src/interfaces/IExternal.sol";
import {MockERC20, MockStockToken, MockPool, MockFeed, MockEngine, StepEngine} from "./mocks/Mocks.sol";

contract ExitlineGuardTest is Test {
    address owner = makeAddr("vaultOwner");
    address curator = makeAddr("curator");
    address guardOwner = makeAddr("guardOwner");
    address keeper = makeAddr("keeper");
    address stranger = makeAddr("stranger");

    uint64 constant GAP = 10 minutes;
    uint256 constant ENGINE_GAS = 1_000_000;
    uint256 constant START_CAP = 1_000_000e6;

    MockERC20 usdg;
    MockStockToken nvda;
    MockPool poolA;
    MockPool poolB;
    MockFeed feed;
    MockEngine engine;
    IVaultV2 vault;
    GuardFactory factory;
    ExitlineGuard guard;

    function setUp() public {
        vm.warp(1_800_000_000);

        usdg = new MockERC20("USDG", 6);
        nvda = new MockStockToken();
        poolA = new MockPool(address(nvda), address(usdg));
        poolB = new MockPool(address(nvda), address(usdg));
        feed = new MockFeed();
        feed.set(block.timestamp);
        engine = new MockEngine();

        vault = IVaultV2(new VaultV2Factory().createVaultV2(owner, address(usdg), bytes32(0)));
        vm.prank(owner);
        vault.setCurator(curator);

        factory = new GuardFactory(engine);
        guard = factory.createGuard(IVaultV2Minimal(address(vault)), guardOwner, GAP, ENGINE_GAS);

        vm.prank(owner);
        vault.setIsSentinel(address(guard), true);

        _raiseCap(START_CAP);

        vm.startPrank(guardOwner);
        guard.setKeeper(keeper, true);
        guard.configure(address(nvda), _config(), _pools());
        vm.stopPrank();
    }

    /* HELPERS */

    function _config() internal view returns (ExitlineGuard.Config memory c) {
        c.coverageBps = 5_000; // lend against half of sellable depth
        c.maxImpactBps = 500; // depth measured to a 5% price fall
        c.closedHaircutBps = 5_000; // halve it again while the market is closed
        c.maxOracleAge = 26 hours;
        c.corporateActionWindow = 2 days;
        c.feed = address(feed);
    }

    function _pools() internal view returns (address[] memory p) {
        p = new address[](2);
        p[0] = address(poolA);
        p[1] = address(poolB);
    }

    function _raiseCap(uint256 cap) internal {
        bytes memory data = abi.encodeCall(IVaultV2.increaseAbsoluteCap, (guard.idData(address(nvda)), cap));
        vm.prank(curator);
        vault.submit(data);
        vm.prank(curator);
        vault.increaseAbsoluteCap(guard.idData(address(nvda)), cap);
    }

    function _cap() internal view returns (uint256) {
        return vault.absoluteCap(guard.capId(address(nvda)));
    }

    /// @dev Sets total depth across both pools and records one reading.
    function _recordWithDepth(uint256 depth) internal returns (uint256 target) {
        engine.set(address(poolA), depth / 2);
        engine.set(address(poolB), depth - depth / 2);
        feed.set(block.timestamp);
        vm.prank(keeper);
        target = guard.record(address(nvda));
        vm.warp(block.timestamp + GAP);
    }

    /* SETUP AND CONFIG */

    function test_idMatchesMorphoAdapterCollateralId() public view {
        // MorphoMarketV1AdapterV2.ids(): keccak256(abi.encode("collateralToken", collateralToken))
        assertEq(guard.capId(address(nvda)), keccak256(abi.encode("collateralToken", address(nvda))));
    }

    function test_constructorRejectsBadGap() public {
        vm.expectRevert(ExitlineGuard.BadParameter.selector);
        new ExitlineGuard(guardOwner, IVaultV2Minimal(address(vault)), engine, 59, ENGINE_GAS);
        vm.expectRevert(ExitlineGuard.BadParameter.selector);
        new ExitlineGuard(guardOwner, IVaultV2Minimal(address(vault)), engine, 1 days + 1, ENGINE_GAS);
    }

    function test_configureDerivesTokenOrder() public view {
        ExitlineGuard.PoolRef[] memory refs = guard.pools(address(nvda));
        assertEq(refs.length, 2);
        assertEq(refs[0].stockIsToken0, poolA.token0() == address(nvda));
    }

    function test_configureRejectsPoolNotQuotedInVaultAsset() public {
        MockERC20 weth = new MockERC20("WETH", 18);
        address[] memory p = new address[](1);
        p[0] = address(new MockPool(address(nvda), address(weth)));
        vm.prank(guardOwner);
        vm.expectRevert(ExitlineGuard.PoolMismatch.selector);
        guard.configure(address(nvda), _config(), p);
    }

    function test_configureRejectsDuplicatePool() public {
        address[] memory p = new address[](2);
        p[0] = address(poolA);
        p[1] = address(poolA);
        vm.prank(guardOwner);
        vm.expectRevert(ExitlineGuard.DuplicatePool.selector);
        guard.configure(address(nvda), _config(), p);
    }

    function test_configureRejectsBadParameters() public {
        ExitlineGuard.Config memory c = _config();
        c.coverageBps = 0;
        vm.prank(guardOwner);
        vm.expectRevert(ExitlineGuard.BadParameter.selector);
        guard.configure(address(nvda), c, _pools());

        c = _config();
        c.maxImpactBps = 5_001;
        vm.prank(guardOwner);
        vm.expectRevert(ExitlineGuard.BadParameter.selector);
        guard.configure(address(nvda), c, _pools());

        c = _config();
        c.closedHaircutBps = 10_001;
        vm.prank(guardOwner);
        vm.expectRevert(ExitlineGuard.BadParameter.selector);
        guard.configure(address(nvda), c, _pools());

        c = _config();
        c.maxOracleAge = 26 hours - 1;
        vm.prank(guardOwner);
        vm.expectRevert(ExitlineGuard.BadParameter.selector);
        guard.configure(address(nvda), c, _pools());
    }

    function test_onlyOwnerConfigures() public {
        vm.prank(stranger);
        vm.expectRevert(abi.encodeWithSelector(Ownable.OwnableUnauthorizedAccount.selector, stranger));
        guard.configure(address(nvda), _config(), _pools());
    }

    function test_reconfigureClearsReadings() public {
        _recordWithDepth(100_000e6);
        _recordWithDepth(100_000e6);
        assertEq(guard.readingCount(address(nvda)), 2);
        vm.prank(guardOwner);
        guard.configure(address(nvda), _config(), _pools());
        assertEq(guard.readingCount(address(nvda)), 0);
    }

    /* RECORDING */

    function test_onlyKeeperRecords() public {
        vm.prank(stranger);
        vm.expectRevert(ExitlineGuard.NotKeeper.selector);
        guard.record(address(nvda));
    }

    function test_recordEnforcesGap() public {
        vm.prank(keeper);
        guard.record(address(nvda));
        vm.warp(block.timestamp + GAP - 1);
        vm.prank(keeper);
        vm.expectRevert(ExitlineGuard.TooSoon.selector);
        guard.record(address(nvda));
    }

    function test_targetIsCoverageOfDepth() public {
        uint256 target = _recordWithDepth(400_000e6);
        assertEq(target, 200_000e6);
    }

    function test_closedMarketAppliesHaircut() public {
        engine.set(address(poolA), 200_000e6);
        engine.set(address(poolB), 200_000e6);
        // setUp runs on a Friday 08:00 UTC, an open weekday: only the stale-feed fallback can close it.
        feed.set(block.timestamp - 26 hours - 1);
        (uint256 target, bool closed,) = guard.preview(address(nvda));
        assertTrue(closed);
        assertEq(target, 100_000e6);
    }

    function test_unreadableFeedCountsAsClosed() public {
        engine.set(address(poolA), 200_000e6);
        engine.set(address(poolB), 200_000e6);
        feed.setBroken(true);
        (uint256 target, bool closed,) = guard.preview(address(nvda));
        assertTrue(closed);
        assertEq(target, 100_000e6);
    }

    function test_unreadablePoolCountsAsZero() public {
        engine.set(address(poolA), 200_000e6);
        engine.set(address(poolB), 200_000e6);
        engine.setReverts(address(poolB), true);
        (uint256 target,, uint256 failed) = guard.preview(address(nvda));
        assertEq(failed, 1);
        assertEq(target, 100_000e6);
    }

    /* CUTTING */

    function test_noCutBeforeFullWindow() public {
        for (uint256 i; i < 4; ++i) {
            _recordWithDepth(10_000e6);
        }
        assertEq(_cap(), START_CAP);
    }

    function test_cutsToMedianOnceWindowIsFull() public {
        uint256[5] memory depths = [uint256(100_000e6), 80_000e6, 120_000e6, 90_000e6, 110_000e6];
        for (uint256 i; i < 5; ++i) {
            _recordWithDepth(depths[i]);
        }
        // Targets are half of depth; median of {50k,40k,60k,45k,55k} is 50k.
        assertEq(_cap(), 50_000e6);
    }

    function test_neverRaisesCap() public {
        for (uint256 i; i < 5; ++i) {
            _recordWithDepth(100_000e6);
        }
        assertEq(_cap(), 50_000e6);
        for (uint256 i; i < 5; ++i) {
            _recordWithDepth(10_000_000e6);
        }
        assertEq(_cap(), 50_000e6, "depth recovering must not raise the cap");
    }

    function test_curatorCanStillRaiseThroughTimelock() public {
        for (uint256 i; i < 5; ++i) {
            _recordWithDepth(100_000e6);
        }
        _raiseCap(START_CAP);
        assertEq(_cap(), START_CAP);
    }

    function test_singleLowReadingCannotTriggerCut() public {
        // A griefer drains the pools for one reading; four honest readings show ample depth.
        _recordWithDepth(4_000_000e6);
        _recordWithDepth(4_000_000e6);
        _recordWithDepth(0);
        _recordWithDepth(4_000_000e6);
        _recordWithDepth(4_000_000e6);
        assertEq(_cap(), START_CAP);
    }

    function test_twoHighReadingsCannotBlockCut() public {
        // An attacker inflates depth for two of five readings; the honest majority still cuts.
        _recordWithDepth(100_000e6);
        _recordWithDepth(1_000_000_000e6);
        _recordWithDepth(100_000e6);
        _recordWithDepth(1_000_000_000e6);
        _recordWithDepth(100_000e6);
        assertEq(_cap(), 50_000e6);
    }

    function test_windowSlides() public {
        for (uint256 i; i < 5; ++i) {
            _recordWithDepth(1_000_000e6);
        }
        assertEq(_cap(), 500_000e6);
        for (uint256 i; i < 3; ++i) {
            _recordWithDepth(200_000e6);
        }
        assertEq(_cap(), 100_000e6, "three new low readings form the new median");
    }

    function test_notSentinelKeepsReadingAndReportsFailure() public {
        vm.prank(owner);
        vault.setIsSentinel(address(guard), false);
        for (uint256 i; i < 4; ++i) {
            _recordWithDepth(100_000e6);
        }
        vm.recordLogs();
        _recordWithDepth(100_000e6);
        Vm.Log[] memory logs = vm.getRecordedLogs();
        bool sawFailure;
        for (uint256 i; i < logs.length; ++i) {
            if (logs[i].emitter == address(guard) && logs[i].topics[0] == ExitlineGuard.CapCutFailed.selector) {
                sawFailure = true;
            }
        }
        assertTrue(sawFailure, "CapCutFailed not emitted");
        assertEq(guard.readingCount(address(nvda)), 5);
        assertEq(_cap(), START_CAP);
    }

    function test_disabledStockCannotBeRecorded() public {
        vm.prank(guardOwner);
        guard.disable(address(nvda));
        vm.prank(keeper);
        vm.expectRevert(ExitlineGuard.NotEnabled.selector);
        guard.record(address(nvda));
    }

    /* EMERGENCY */

    function test_emergencyCutRevertsWithoutCorporateAction() public {
        vm.prank(stranger);
        vm.expectRevert(ExitlineGuard.NoCorporateAction.selector);
        guard.emergencyCut(address(nvda));
    }

    function test_emergencyCutWithinWindowZeroesCap() public {
        nvda.scheduleMultiplier(2e18, block.timestamp + 1 days);
        vm.prank(stranger);
        guard.emergencyCut(address(nvda));
        assertEq(_cap(), 0);
    }

    function test_emergencyCutIgnoresDistantCorporateAction() public {
        nvda.scheduleMultiplier(2e18, block.timestamp + 30 days);
        vm.expectRevert(ExitlineGuard.NoCorporateAction.selector);
        guard.emergencyCut(address(nvda));
    }

    function test_emergencyCutRevertsIfNotSentinel() public {
        vm.prank(owner);
        vault.setIsSentinel(address(guard), false);
        nvda.scheduleMultiplier(2e18, block.timestamp + 1 days);
        vm.expectRevert(ErrorsLib.Unauthorized.selector);
        guard.emergencyCut(address(nvda));
    }

    function test_emergencyCutOnTokenWithoutMultiplierReverts() public {
        // A plain ERC20 without ERC-8056 functions never counts as a pending corporate action.
        MockERC20 plain = new MockERC20("PLAIN", 18);
        address[] memory p = new address[](1);
        p[0] = address(new MockPool(address(plain), address(usdg)));
        vm.prank(guardOwner);
        guard.configure(address(plain), _config(), p);
        vm.expectRevert(ExitlineGuard.NoCorporateAction.selector);
        guard.emergencyCut(address(plain));
    }

    /* OWNERSHIP AND FACTORY */

    function test_ownershipIsTwoStep() public {
        vm.prank(guardOwner);
        guard.transferOwnership(stranger);
        assertEq(guard.owner(), guardOwner);
        vm.prank(stranger);
        guard.acceptOwnership();
        assertEq(guard.owner(), stranger);
    }

    function test_factoryRecordsGuard() public view {
        assertTrue(factory.isGuard(address(guard)));
        assertEq(factory.guardsByVault(address(vault))[0], address(guard));
        assertEq(address(guard.engine()), address(engine));
        assertEq(guard.asset(), address(usdg));
    }

    /* FUZZ */

    function testFuzz_cutNeverExceedsOldCapAndEqualsMedian(uint128[5] memory depths) public {
        uint256[5] memory targets;
        for (uint256 i; i < 5; ++i) {
            uint256 d = bound(uint256(depths[i]), 0, 1e15 * 1e6);
            targets[i] = _recordWithDepth(d);
        }
        // Sort targets to find the expected median.
        for (uint256 i = 1; i < 5; ++i) {
            for (uint256 j = i; j > 0 && targets[j - 1] > targets[j]; --j) {
                (targets[j - 1], targets[j]) = (targets[j], targets[j - 1]);
            }
        }
        uint256 expected = targets[2] < START_CAP ? targets[2] : START_CAP;
        assertEq(_cap(), expected);
        assertLe(_cap(), START_CAP);
    }

    /* ENGINE GAS BUDGET */

    /// @dev A guard on the same vault whose engine is a StepEngine, configured like `guard`.
    function _stepGuard(uint256 engineGas) internal returns (ExitlineGuard g, StepEngine e) {
        e = new StepEngine();
        g = new GuardFactory(e).createGuard(IVaultV2Minimal(address(vault)), guardOwner, GAP, engineGas);
        vm.prank(owner);
        vault.setIsSentinel(address(g), true);
        vm.startPrank(guardOwner);
        g.setKeeper(keeper, true);
        g.configure(address(nvda), _config(), _pools());
        vm.stopPrank();
    }

    /// @dev Smallest gas a keeper can send to `record` (or anyone to `preview`) that passes the
    /// entry check: probe with gasRequired, then add the shortfall the revert reports. Gas used
    /// before the check is deterministic, so this is the exact threshold.
    function _minimumGas(ExitlineGuard g, bool isRecord) internal returns (uint256) {
        uint256 need = g.gasRequired(address(nvda));
        bytes memory data = isRecord
            ? abi.encodeCall(ExitlineGuard.record, (address(nvda)))
            : abi.encodeCall(ExitlineGuard.preview, (address(nvda)));
        if (isRecord) vm.prank(keeper);
        (bool ok, bytes memory ret) = address(g).call{gas: need}(data);
        require(!ok && bytes4(ret) == ExitlineGuard.InsufficientGas.selector, "probe did not hit the gas check");
        (uint256 required, uint256 available) = abi.decode(_tail(ret), (uint256, uint256));
        return need + (required - available);
    }

    function _tail(bytes memory ret) internal pure returns (bytes memory out) {
        out = new bytes(ret.length - 4);
        for (uint256 i; i < out.length; ++i) {
            out[i] = ret[i + 4];
        }
    }

    function test_constructorBoundsEngineGas() public {
        uint256 lo = guard.MIN_ENGINE_GAS();
        uint256 hi = guard.MAX_ENGINE_GAS();
        vm.expectRevert(ExitlineGuard.BadParameter.selector);
        new ExitlineGuard(guardOwner, IVaultV2Minimal(address(vault)), engine, GAP, lo - 1);
        vm.expectRevert(ExitlineGuard.BadParameter.selector);
        new ExitlineGuard(guardOwner, IVaultV2Minimal(address(vault)), engine, GAP, hi + 1);
        new ExitlineGuard(guardOwner, IVaultV2Minimal(address(vault)), engine, GAP, lo);
        new ExitlineGuard(guardOwner, IVaultV2Minimal(address(vault)), engine, GAP, hi);
    }

    function test_factoryPassesEngineGasLimit() public view {
        assertEq(guard.engineGasLimit(), ENGINE_GAS);
    }

    function test_worstCaseFitsInTransactionGasLimit() public view {
        uint256 perPool = guard.MAX_ENGINE_GAS() + (guard.MAX_ENGINE_GAS() + 62) / 63 + guard.ENGINE_CALL_GAS();
        assertLe(guard.MAX_POOLS() * perPool + guard.RECORD_GAS_OVERHEAD(), 32_000_000);
    }

    function test_gasRequiredCoversEveryPoolBudget() public view {
        uint256 need = guard.gasRequired(address(nvda));
        assertGe(need, 2 * ENGINE_GAS + guard.RECORD_GAS_OVERHEAD());
    }

    function test_keeperWithTooLittleGasRevertsAndRecordsNothing() public {
        engine.set(address(poolA), 1_000_000e6);
        engine.set(address(poolB), 1_000_000e6);
        uint256 min = _minimumGas(guard, true);
        vm.prank(keeper);
        vm.expectPartialRevert(ExitlineGuard.InsufficientGas.selector);
        guard.record{gas: min - 1}(address(nvda));
        assertEq(guard.readingCount(address(nvda)), 0);
        assertEq(guard.readings(address(nvda)).length, 0);
    }

    function test_previewWithTooLittleGasReverts() public {
        uint256 min = _minimumGas(guard, false);
        vm.expectPartialRevert(ExitlineGuard.InsufficientGas.selector);
        guard.preview{gas: min - 1}(address(nvda));
        guard.preview{gas: min}(address(nvda));
    }

    function test_engineOverBudgetCountsAsZeroDepth() public {
        (ExitlineGuard g, StepEngine e) = _stepGuard(ENGINE_GAS);
        // 20 steps of 100k gas: twice the 1M budget.
        e.set(20, 100_000, 1_000_000e6);
        (uint256 target,, uint256 failed) = g.preview(address(nvda));
        assertEq(failed, 2);
        assertEq(target, 0);
    }

    function test_engineWithinBudgetCounts() public {
        (ExitlineGuard g, StepEngine e) = _stepGuard(ENGINE_GAS);
        // 8 steps of 100k gas: 800k, inside the 1M budget.
        e.set(8, 100_000, 1_000_000e6);
        (uint256 target,, uint256 failed) = g.preview(address(nvda));
        assertEq(failed, 0);
        assertEq(target, 2 * 1_000_000e6 * 5_000 / 10_000);
    }

    /// @dev The worst case at the minimum allowed gas: every pool burns its whole budget, and the
    /// reading must still be stored and the cap cut through the real Vault V2 code. If the fixed
    /// overhead were too small, this would run out of gas or record a CapCutFailed.
    function test_minimumGasWithFullBudgetBurnStillRecordsAndCuts() public {
        (ExitlineGuard g, StepEngine e) = _stepGuard(ENGINE_GAS);
        // More work than the budget: each engine call burns all of it and fails.
        e.set(1_000, 100_000, 0);
        for (uint256 i; i < g.WINDOW(); ++i) {
            feed.set(block.timestamp);
            uint256 need = _minimumGas(g, true);
            vm.recordLogs();
            vm.prank(keeper);
            g.record{gas: need}(address(nvda));
            Vm.Log[] memory logs = vm.getRecordedLogs();
            for (uint256 j; j < logs.length; ++j) {
                assertTrue(logs[j].topics[0] != ExitlineGuard.CapCutFailed.selector, "cap cut starved");
            }
            vm.warp(block.timestamp + GAP);
        }
        assertEq(g.readingCount(address(nvda)), g.WINDOW());
        assertEq(_cap(), 0, "cap cut to zero depth");
    }

    function test_normalCallAtMinimumGasMatchesAmpleGas() public {
        engine.set(address(poolA), 300_000e6);
        engine.set(address(poolB), 500_000e6);
        (uint256 ample,,) = guard.preview(address(nvda));
        uint256 need = _minimumGas(guard, true);
        vm.prank(keeper);
        uint256 target = guard.record{gas: need}(address(nvda));
        assertEq(target, ample);
        assertEq(target, 800_000e6 * 5_000 / 10_000);
    }

    /* WEEKEND CALENDAR */

    // Timestamps checked independently (Python datetime, UTC). Oct 2026 is EDT, Dec 2026 is EST.
    uint256 constant THU_1200_UTC = 1_790_856_000; // Thu 2026-10-01 12:00
    uint256 constant FRI_1500_UTC = 1_790_953_200; // Fri 2026-10-02 15:00 (11:00 EDT)
    uint256 constant FRI_2359_UTC = 1_790_985_599; // Fri 2026-10-02 23:59:59 (19:59:59 EDT)
    uint256 constant SAT_0000_UTC = 1_790_985_600; // Sat 2026-10-03 00:00 (Fri 20:00 EDT)
    uint256 constant SUN_2359_UTC = 1_791_158_399; // Sun 2026-10-04 23:59:59
    uint256 constant MON_0000_UTC = 1_791_158_400; // Mon 2026-10-05 00:00 (Sun 20:00 EDT)
    uint256 constant MON_0030_UTC = 1_791_160_200; // Mon 2026-10-05 00:30
    uint256 constant MON_0059_UTC = 1_791_161_999; // Mon 2026-10-05 00:59:59
    uint256 constant MON_0100_UTC = 1_791_162_000; // Mon 2026-10-05 01:00
    uint256 constant EST_FRI_CLOSE = 1_796_432_400; // Sat 2026-12-05 01:00 UTC = Fri 20:00 EST
    uint256 constant EST_SUN_OPEN = 1_796_605_200; // Mon 2026-12-07 01:00 UTC = Sun 20:00 EST

    function test_weekendWindowEdges() public view {
        assertFalse(guard.isWeekendClosed(THU_1200_UTC), "Thursday");
        assertFalse(guard.isWeekendClosed(FRI_1500_UTC), "Friday afternoon");
        assertFalse(guard.isWeekendClosed(FRI_2359_UTC), "last second of Friday UTC");
        assertTrue(guard.isWeekendClosed(SAT_0000_UTC), "Saturday 00:00");
        assertTrue(guard.isWeekendClosed(SUN_2359_UTC), "Sunday 23:59:59");
        assertTrue(guard.isWeekendClosed(MON_0000_UTC), "Monday 00:00");
        assertTrue(guard.isWeekendClosed(MON_0030_UTC), "Monday 00:30");
        assertTrue(guard.isWeekendClosed(MON_0059_UTC), "Monday 00:59:59");
        assertFalse(guard.isWeekendClosed(MON_0100_UTC), "Monday 01:00");
    }

    function test_weekendWindowCoversEstHours() public view {
        // Under EST the session ends Fri 20:00 = Sat 01:00 UTC and resumes Sun 20:00 = Mon 01:00 UTC.
        assertTrue(guard.isWeekendClosed(EST_FRI_CLOSE - 1), "Fri 19:59:59 EST, after the UTC close");
        assertTrue(guard.isWeekendClosed(EST_FRI_CLOSE));
        assertTrue(guard.isWeekendClosed(EST_SUN_OPEN - 1));
        assertFalse(guard.isWeekendClosed(EST_SUN_OPEN), "Sun 20:00 EST reopen");
    }

    function testFuzz_weekendIsExactly49HoursPerWeek(uint32 weekIndex) public view {
        // Every week from a Monday 01:00 UTC holds Sat 00:00 .. Mon 01:00: 49 hours closed.
        uint256 start = MON_0100_UTC + uint256(weekIndex % 2_000) * 7 days;
        uint256 closedHours;
        for (uint256 h; h < 7 * 24; ++h) {
            if (guard.isWeekendClosed(start + h * 1 hours)) ++closedHours;
        }
        assertEq(closedHours, 49);
    }

    function _previewAt(uint256 ts, uint256 feedAge) internal returns (bool closed, uint256 target) {
        vm.warp(ts);
        engine.set(address(poolA), 200_000e6);
        engine.set(address(poolB), 200_000e6);
        feed.set(ts - feedAge);
        (target, closed,) = guard.preview(address(nvda));
    }

    function test_saturdayWithFreshFeedIsClosed() public {
        (bool closed, uint256 target) = _previewAt(SAT_0000_UTC, 0);
        assertTrue(closed);
        assertTrue(guard.marketClosed(address(nvda)));
        assertEq(target, 100_000e6);
    }

    function test_mondayHalfPastMidnightUtcIsClosed() public {
        (bool closed,) = _previewAt(MON_0030_UTC, 0);
        assertTrue(closed);
    }

    function test_mondayOneAmUtcReopens() public {
        (bool closed, uint256 target) = _previewAt(MON_0100_UTC, 0);
        assertFalse(closed);
        assertEq(target, 200_000e6);
    }

    function test_fridayAfternoonUtcWithQuietFeedIsOpen() public {
        // A feed 25 h old on a weekday is a quiet stock, not a closed market.
        (bool closed, uint256 target) = _previewAt(FRI_1500_UTC, 25 hours);
        assertFalse(closed);
        assertEq(target, 200_000e6);
    }

    function test_weekdayStaleFeedFallbackCloses() public {
        (bool closed,) = _previewAt(THU_1200_UTC, 26 hours + 1);
        assertTrue(closed);
    }

    function test_weekdayUnreadableFeedFallbackCloses() public {
        vm.warp(THU_1200_UTC);
        feed.setBroken(true);
        assertTrue(guard.marketClosed(address(nvda)));
    }
}
