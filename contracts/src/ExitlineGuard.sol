// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {Ownable, Ownable2Step} from "@openzeppelin/contracts/access/Ownable2Step.sol";

import {IDepthEngine} from "./interfaces/IDepthEngine.sol";
import {IVaultV2Minimal, IUniswapV3PoolTokens, IStockToken, IAggregatorV3} from "./interfaces/IExternal.sol";

/// @title ExitlineGuard
/// @notice Lowers a Morpho Vault V2 vault's per-stock lending cap when the stock can no longer be
/// sold onchain in the size the vault has lent against it.
/// @dev The guard is installed as a Vault V2 *sentinel*. Vault V2 lets a sentinel lower caps
/// instantly but never raise them; raising stays behind the curator's timelock. This contract
/// never calls any other sentinel power (revoke, deallocate), so the worst it can do is block
/// new lending into a stock.
///
/// Readings are recorded by keepers, spaced at least `minGap` apart. A cap is cut to the median
/// of the last WINDOW readings, so a single manipulated reading can neither trigger nor block a cut.
/// Recording is keeper-gated because pool liquidity can be added and removed inside one
/// transaction; letting anyone record at a moment of their choosing would let an attacker fill
/// the window with manipulated readings.
///
/// Every engine call gets exactly `engineGasLimit`, and `record` and `preview` refuse to start
/// unless the caller supplied enough gas for every pool's full budget plus the work after it. So a
/// pool counted as failed always failed within its full budget; a keeper cannot starve the engine
/// (or the cap cut) to push a reading down or up.
contract ExitlineGuard is Ownable2Step {
    /* CONSTANTS */

    uint256 public constant WINDOW = 5;
    uint256 public constant BPS = 10_000;
    uint256 public constant MAX_POOLS = 8;
    uint256 public constant MAX_IMPACT_BPS = 5_000;
    uint64 public constant MIN_GAP_FLOOR = 60;
    uint64 public constant MIN_GAP_CEILING = 1 days;
    /// @dev Floor for `maxOracleAge`. Robinhood's Chainlink stock feeds have a 24 h heartbeat and a
    /// 0.5% deviation trigger, so a quiet weekday can leave a feed nearly a day old.
    uint32 public constant MIN_ORACLE_AGE = 26 hours;
    /// @dev Weekend closure in UTC: Saturday 00:00 to Monday 01:00. US equities close Friday 20:00
    /// and reopen Sunday 20:00 New York time (24/5 feed hours), which is Sat 00:00 / Mon 00:00 UTC
    /// under EDT and Sat 01:00 / Mon 01:00 UTC under EST. The window covers both and overlaps
    /// trading by at most an hour, on the side that applies the stricter haircut.
    uint256 internal constant MONDAY_REOPEN = 1 hours;

    /// @dev Bounds on the per-pool engine budget. The ceiling keeps a full measurement
    /// (MAX_POOLS × (budget + 1/63 + call cost) + overhead) under the 32M per-transaction gas
    /// limit of Arbitrum Nitro chains.
    uint256 public constant MIN_ENGINE_GAS = 100_000;
    uint256 public constant MAX_ENGINE_GAS = 3_500_000;
    /// @dev Per pool, on top of the budget: cold account access, call base cost, ABI work.
    uint256 public constant ENGINE_CALL_GAS = 10_000;
    /// @dev Everything outside the engine calls: config and pool reads, the oracle read, the
    /// reading's storage writes, the median, and the vault cap read and cut.
    uint256 public constant RECORD_GAS_OVERHEAD = 250_000;

    /* TYPES */

    struct Config {
        bool enabled;
        /// @dev Share of sellable depth the vault may lend against, in bps (1..10000).
        uint16 coverageBps;
        /// @dev Price fall allowed when measuring depth, in bps (1..MAX_IMPACT_BPS).
        uint16 maxImpactBps;
        /// @dev Extra multiplier on depth while the stock's oracle is stale (market closed), in bps (0..10000).
        uint16 closedHaircutBps;
        /// @dev Oracle age beyond which the market is treated as closed, as a fallback to the
        /// weekend calendar (>= MIN_ORACLE_AGE).
        uint32 maxOracleAge;
        /// @dev How far ahead a pending corporate action triggers an emergency cut.
        uint32 corporateActionWindow;
        /// @dev Chainlink feed for the stock.
        address feed;
    }

    struct PoolRef {
        address pool;
        bool stockIsToken0;
    }

    struct Reading {
        uint64 timestamp;
        uint192 target;
    }

    /* IMMUTABLES */

    IVaultV2Minimal public immutable vault;
    address public immutable asset;
    IDepthEngine public immutable engine;
    uint64 public immutable minGap;
    /// @notice Gas given to each engine call.
    uint256 public immutable engineGasLimit;

    /* STORAGE */

    mapping(address account => bool) public isKeeper;
    mapping(address stock => Config) internal _config;
    mapping(address stock => PoolRef[]) internal _pools;
    mapping(address stock => Reading[WINDOW]) internal _readings;
    mapping(address stock => uint256) public readingCount;

    /* EVENTS */

    event SetKeeper(address indexed account, bool isKeeper);
    event Configured(address indexed stock, Config config, PoolRef[] pools);
    event Disabled(address indexed stock);
    event Recorded(address indexed stock, uint256 target, bool marketClosed, uint256 failedPools);
    event CapCut(address indexed stock, uint256 oldCap, uint256 newCap, bool emergency);
    event CapCutFailed(address indexed stock, uint256 attemptedCap, bytes reason);

    /* ERRORS */

    error ZeroAddress();
    error BadParameter();
    error NotKeeper();
    error NotEnabled();
    error TooSoon();
    error PoolMismatch();
    error DuplicatePool();
    error NoCorporateAction();
    error InsufficientGas(uint256 required, uint256 available);

    /* CONSTRUCTOR */

    constructor(
        address initialOwner,
        IVaultV2Minimal _vault,
        IDepthEngine _engine,
        uint64 _minGap,
        uint256 _engineGasLimit
    ) Ownable(initialOwner) {
        if (address(_vault) == address(0) || address(_engine) == address(0)) revert ZeroAddress();
        if (_minGap < MIN_GAP_FLOOR || _minGap > MIN_GAP_CEILING) revert BadParameter();
        if (_engineGasLimit < MIN_ENGINE_GAS || _engineGasLimit > MAX_ENGINE_GAS) revert BadParameter();
        vault = _vault;
        asset = _vault.asset();
        engine = _engine;
        minGap = _minGap;
        engineGasLimit = _engineGasLimit;
    }

    /* MODIFIERS */

    modifier onlyKeeper() {
        if (!isKeeper[msg.sender] && msg.sender != owner()) revert NotKeeper();
        _;
    }

    /* OWNER FUNCTIONS */

    function setKeeper(address account, bool newIsKeeper) external onlyOwner {
        if (account == address(0)) revert ZeroAddress();
        isKeeper[account] = newIsKeeper;
        emit SetKeeper(account, newIsKeeper);
    }

    /// @notice Sets the parameters and pools for a stock. Clears its readings, since readings taken
    /// under old parameters are not comparable.
    /// @dev Every pool must pair the stock with the vault asset. Pools quoted in anything else are
    /// rejected rather than converted, so depth is always measured in the asset the vault lends.
    function configure(address stock, Config calldata cfg, address[] calldata poolList) external onlyOwner {
        if (stock == address(0) || cfg.feed == address(0)) revert ZeroAddress();
        if (stock == asset) revert BadParameter();
        if (cfg.coverageBps == 0 || cfg.coverageBps > BPS) revert BadParameter();
        if (cfg.maxImpactBps == 0 || cfg.maxImpactBps > MAX_IMPACT_BPS) revert BadParameter();
        if (cfg.closedHaircutBps > BPS) revert BadParameter();
        if (cfg.maxOracleAge < MIN_ORACLE_AGE) revert BadParameter();
        if (poolList.length == 0 || poolList.length > MAX_POOLS) revert BadParameter();

        delete _pools[stock];
        PoolRef[] storage refs = _pools[stock];
        for (uint256 i; i < poolList.length; ++i) {
            address pool = poolList[i];
            for (uint256 j; j < i; ++j) {
                if (poolList[j] == pool) revert DuplicatePool();
            }
            address t0 = IUniswapV3PoolTokens(pool).token0();
            address t1 = IUniswapV3PoolTokens(pool).token1();
            bool stockIsToken0;
            if (t0 == stock && t1 == asset) stockIsToken0 = true;
            else if (t1 == stock && t0 == asset) stockIsToken0 = false;
            else revert PoolMismatch();
            refs.push(PoolRef({pool: pool, stockIsToken0: stockIsToken0}));
        }

        Config memory stored = cfg;
        stored.enabled = true;
        _config[stock] = stored;
        readingCount[stock] = 0;

        emit Configured(stock, stored, refs);
    }

    function disable(address stock) external onlyOwner {
        _config[stock].enabled = false;
        readingCount[stock] = 0;
        emit Disabled(stock);
    }

    /* KEEPER FUNCTIONS */

    /// @notice Measures the stock's current safe cap, stores it, and cuts the vault cap once
    /// WINDOW readings exist and their median is below the current cap.
    function record(address stock) external onlyKeeper returns (uint256 target) {
        _requireGas(stock);
        Config memory cfg = _config[stock];
        if (!cfg.enabled) revert NotEnabled();

        uint256 count = readingCount[stock];
        if (count > 0) {
            Reading memory last = _readings[stock][(count - 1) % WINDOW];
            if (block.timestamp < uint256(last.timestamp) + minGap) revert TooSoon();
        }

        bool closed;
        uint256 failed;
        (target, closed, failed) = _measure(stock, cfg);

        _readings[stock][count % WINDOW] = Reading({timestamp: uint64(block.timestamp), target: uint192(target)});
        readingCount[stock] = count + 1;
        emit Recorded(stock, target, closed, failed);

        if (count + 1 >= WINDOW) {
            _cutTo(stock, _median(stock), false);
        }
    }

    /* PERMISSIONLESS FUNCTIONS */

    /// @notice Cuts the stock's cap to zero while a corporate action (split, reverse split,
    /// dividend adjustment) is pending. Reads only issuer-controlled token state, so anyone may call.
    function emergencyCut(address stock) external {
        Config memory cfg = _config[stock];
        if (!cfg.enabled) revert NotEnabled();
        if (!_corporateActionPending(stock, cfg.corporateActionWindow)) revert NoCorporateAction();
        _cutTo(stock, 0, true);
    }

    /* VIEWS */

    function idData(address stock) public pure returns (bytes memory) {
        return abi.encode("collateralToken", stock);
    }

    function capId(address stock) public pure returns (bytes32) {
        return keccak256(idData(stock));
    }

    function config(address stock) external view returns (Config memory) {
        return _config[stock];
    }

    function pools(address stock) external view returns (PoolRef[] memory) {
        return _pools[stock];
    }

    function readings(address stock) external view returns (Reading[] memory out) {
        uint256 count = readingCount[stock];
        uint256 n = count < WINDOW ? count : WINDOW;
        out = new Reading[](n);
        for (uint256 i; i < n; ++i) {
            out[i] = _readings[stock][(count - n + i) % WINDOW];
        }
    }

    /// @notice What `record` would store right now, without storing it.
    function preview(address stock) external view returns (uint256 target, bool closed, uint256 failedPools) {
        _requireGas(stock);
        Config memory cfg = _config[stock];
        if (!cfg.enabled) revert NotEnabled();
        return _measure(stock, cfg);
    }

    /// @notice Minimum gasleft() at entry for `record` and `preview` on this stock. Keepers should
    /// send at least this much plus the transaction's intrinsic cost.
    function gasRequired(address stock) public view returns (uint256) {
        uint256 n = _pools[stock].length;
        // EIP-150: a call receives at most 63/64 of the gas left, so reserve a 64th on top.
        uint256 perPool = engineGasLimit + (engineGasLimit + 62) / 63 + ENGINE_CALL_GAS;
        return n * perPool + RECORD_GAS_OVERHEAD;
    }

    /// @notice Whether `record` would treat the stock's market as closed right now.
    function marketClosed(address stock) external view returns (bool) {
        return _marketClosed(_config[stock]);
    }

    /// @notice True from Saturday 00:00 UTC to Monday 01:00 UTC. US market holidays are not covered.
    function isWeekendClosed(uint256 timestamp) public pure returns (bool) {
        // 1970-01-01 was a Thursday, so (days + 4) % 7 gives 0 = Sunday ... 6 = Saturday.
        uint256 weekday = (timestamp / 1 days + 4) % 7;
        if (weekday == 6 || weekday == 0) return true;
        return weekday == 1 && timestamp % 1 days < MONDAY_REOPEN;
    }

    function corporateActionPending(address stock) external view returns (bool) {
        return _corporateActionPending(stock, _config[stock].corporateActionWindow);
    }

    /* INTERNAL */

    function _requireGas(address stock) internal view {
        uint256 required = gasRequired(stock);
        uint256 available = gasleft();
        if (available < required) revert InsufficientGas(required, available);
    }

    function _measure(address stock, Config memory cfg)
        internal
        view
        returns (uint256 target, bool closed, uint256 failed)
    {
        PoolRef[] storage refs = _pools[stock];
        uint256 depth;
        for (uint256 i; i < refs.length; ++i) {
            PoolRef memory ref = refs[i];
            // A pool the engine cannot read within its budget counts as zero depth: failing safe
            // means cutting, not ignoring. The entry gas check guarantees the full budget here.
            try engine.sellProceeds{gas: engineGasLimit}(ref.pool, ref.stockIsToken0, cfg.maxImpactBps) returns (
                uint256 proceeds
            ) {
                depth += proceeds > type(uint128).max ? type(uint128).max : proceeds;
            } catch {
                ++failed;
            }
        }

        target = depth * cfg.coverageBps / BPS;
        closed = _marketClosed(cfg);
        if (closed) target = target * cfg.closedHaircutBps / BPS;
        if (target > type(uint128).max) target = type(uint128).max;
    }

    /// @dev Closed on the weekend calendar. As a fallback, also closed if the feed cannot be read
    /// or its last update is older than `maxOracleAge`. Closed applies the stricter haircut.
    function _marketClosed(Config memory cfg) internal view returns (bool) {
        if (isWeekendClosed(block.timestamp)) return true;
        try IAggregatorV3(cfg.feed).latestRoundData() returns (uint80, int256, uint256, uint256 updatedAt, uint80) {
            if (updatedAt > block.timestamp) return false;
            return block.timestamp - updatedAt > cfg.maxOracleAge;
        } catch {
            return true;
        }
    }

    function _corporateActionPending(address stock, uint256 window) internal view returns (bool) {
        IStockToken token = IStockToken(stock);
        try token.newUIMultiplier() returns (uint256 pending) {
            try token.uiMultiplier() returns (uint256 current) {
                if (pending == current) return false;
                try token.effectiveAt() returns (uint256 at) {
                    return at <= block.timestamp + window;
                } catch {
                    return false;
                }
            } catch {
                return false;
            }
        } catch {
            return false;
        }
    }

    function _median(address stock) internal view returns (uint256) {
        uint256[WINDOW] memory values;
        for (uint256 i; i < WINDOW; ++i) {
            values[i] = _readings[stock][i].target;
        }
        // Insertion sort over five values.
        for (uint256 i = 1; i < WINDOW; ++i) {
            uint256 v = values[i];
            uint256 j = i;
            while (j > 0 && values[j - 1] > v) {
                values[j] = values[j - 1];
                --j;
            }
            values[j] = v;
        }
        return values[WINDOW / 2];
    }

    /// @dev Only ever lowers. On the reading path a failed vault call is reported, not reverted,
    /// so the reading is kept. On the emergency path it reverts, so the caller sees the failure.
    function _cutTo(address stock, uint256 newCap, bool emergency) internal {
        uint256 oldCap = vault.absoluteCap(capId(stock));
        if (newCap >= oldCap) return;
        if (emergency) {
            vault.decreaseAbsoluteCap(idData(stock), newCap);
            emit CapCut(stock, oldCap, newCap, true);
            return;
        }
        try vault.decreaseAbsoluteCap(idData(stock), newCap) {
            emit CapCut(stock, oldCap, newCap, emergency);
        } catch (bytes memory reason) {
            emit CapCutFailed(stock, newCap, reason);
        }
    }
}
