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
import {MockERC20, MockStockToken, MockPool, MockFeed, MockEngine} from "./mocks/Mocks.sol";

contract ExitlineGuardTest is Test {
    address owner = makeAddr("vaultOwner");
    address curator = makeAddr("curator");
    address guardOwner = makeAddr("guardOwner");
    address keeper = makeAddr("keeper");
    address stranger = makeAddr("stranger");

    uint64 constant GAP = 10 minutes;
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
        guard = factory.createGuard(IVaultV2Minimal(address(vault)), guardOwner, GAP);

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
        c.maxOracleAge = 1 hours;
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
        new ExitlineGuard(guardOwner, IVaultV2Minimal(address(vault)), engine, 59);
        vm.expectRevert(ExitlineGuard.BadParameter.selector);
        new ExitlineGuard(guardOwner, IVaultV2Minimal(address(vault)), engine, 1 days + 1);
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
        feed.set(block.timestamp - 1 hours - 1);
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
}
