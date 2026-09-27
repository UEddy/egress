// SPDX-License-Identifier: MIT
pragma solidity 0.7.6;
pragma abicoder v2;

import "forge-std/Test.sol";

import {UniswapV3Factory} from "v3-core/UniswapV3Factory.sol";
import {IUniswapV3Pool} from "v3-core/interfaces/IUniswapV3Pool.sol";
import {TickMath} from "v3-core/libraries/TickMath.sol";

/// Minimal ERC20 for 0.7.6.
contract Token {
    string public name;
    uint8 public decimals;
    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;

    constructor(string memory _name, uint8 _decimals) {
        name = _name;
        decimals = _decimals;
    }

    function mint(address to, uint256 amount) external {
        balanceOf[to] += amount;
    }

    function transfer(address to, uint256 amount) external returns (bool) {
        require(balanceOf[msg.sender] >= amount, "balance");
        balanceOf[msg.sender] -= amount;
        balanceOf[to] += amount;
        return true;
    }
}

/// Pays the pool in mint and swap callbacks from its own balance.
contract Actor {
    function mint(IUniswapV3Pool pool, int24 lower, int24 upper, uint128 amount) external {
        pool.mint(address(this), lower, upper, amount, "");
    }

    function sell(IUniswapV3Pool pool, bool zeroForOne, uint160 limit) external returns (uint256 out) {
        (int256 a0, int256 a1) = pool.swap(address(this), zeroForOne, int256(1e40), limit, "");
        out = uint256(zeroForOne ? -a1 : -a0);
    }

    function uniswapV3MintCallback(uint256 owed0, uint256 owed1, bytes calldata) external {
        IUniswapV3Pool pool = IUniswapV3Pool(msg.sender);
        if (owed0 > 0) Token(pool.token0()).transfer(msg.sender, owed0);
        if (owed1 > 0) Token(pool.token1()).transfer(msg.sender, owed1);
    }

    function uniswapV3SwapCallback(int256 d0, int256 d1, bytes calldata) external {
        IUniswapV3Pool pool = IUniswapV3Pool(msg.sender);
        if (d0 > 0) Token(pool.token0()).transfer(msg.sender, uint256(d0));
        if (d1 > 0) Token(pool.token1()).transfer(msg.sender, uint256(d1));
    }
}

/// Builds real Uniswap V3 pools, sells into them up to a price limit, and writes the pool state
/// plus the real swap output to ../engine/fixtures. The Rust engine test replays the same state
/// and must report no more than the real output, and at most a few wei less.
contract CrossCheck is Test {
    UniswapV3Factory factory;
    Token a;
    Token b;
    Actor actor;

    int24[] minted;

    function setUp() public {
        factory = new UniswapV3Factory();
        factory.enableFeeAmount(100, 1);
        a = new Token("STOCK", 18);
        b = new Token("USDG", 6);
        actor = new Actor();
        a.mint(address(actor), 1e45);
        b.mint(address(actor), 1e45);
    }

    function _pool(uint24 fee, int24 startTick) internal returns (IUniswapV3Pool pool) {
        pool = IUniswapV3Pool(factory.createPool(address(a), address(b), fee));
        pool.initialize(TickMath.getSqrtRatioAtTick(startTick));
        delete minted;
    }

    function _mint(IUniswapV3Pool pool, int24 lower, int24 upper, uint128 amount) internal {
        actor.mint(pool, lower, upper, amount);
        minted.push(lower);
        minted.push(upper);
    }

    function _state(IUniswapV3Pool pool) internal view returns (string memory json) {
        (uint160 sqrtP, int24 tick,,,,,) = pool.slot0();
        int24 spacing = pool.tickSpacing();
        string memory words = "";
        string memory ticks = "";
        for (uint256 i; i < minted.length; ++i) {
            int24 t = minted[i];
            (, int128 net,,,,,,) = pool.ticks(t);
            ticks = string(
                abi.encodePacked(ticks, i == 0 ? "" : ",", '{"tick":', vm.toString(int256(t)), ',"net":"', vm.toString(int256(net)), '"}')
            );
            int16 pos = int16((t / spacing) >> 8);
            words = string(
                abi.encodePacked(
                    words, i == 0 ? "" : ",", '{"pos":', vm.toString(int256(pos)), ',"word":"', vm.toString(pool.tickBitmap(pos)), '"}'
                )
            );
        }
        json = string(
            abi.encodePacked(
                '{"sqrt_price":"', vm.toString(uint256(sqrtP)),
                '","tick":', vm.toString(int256(tick)),
                ',"liquidity":"', vm.toString(uint256(pool.liquidity())),
                '","spacing":', vm.toString(int256(spacing)),
                ',"words":[', words, '],"ticks":[', ticks, ']'
            )
        );
    }

    function _case(IUniswapV3Pool pool, bool zeroForOne, uint160 limit) internal returns (string memory) {
        uint256 snap = vm.snapshot();
        uint256 out = actor.sell(pool, zeroForOne, limit);
        vm.revertTo(snap);
        return string(
            abi.encodePacked(
                '{"zero_for_one":', zeroForOne ? "true" : "false",
                ',"limit":"', vm.toString(uint256(limit)),
                '","amount_out":"', vm.toString(out), '"}'
            )
        );
    }

    function _run(string memory name, IUniswapV3Pool pool) internal {
        (uint160 sqrtP,,,,,,) = pool.slot0();
        string memory s = _state(pool);
        string memory cases = string(
            abi.encodePacked(
                _case(pool, true, uint160(uint256(sqrtP) * 99 / 100)), ",",
                _case(pool, true, uint160(uint256(sqrtP) * 95 / 100)), ",",
                _case(pool, true, uint160(uint256(sqrtP) * 80 / 100)), ",",
                _case(pool, false, uint160(uint256(sqrtP) * 101 / 100)), ",",
                _case(pool, false, uint160(uint256(sqrtP) * 105 / 100)), ",",
                _case(pool, false, uint160(uint256(sqrtP) * 120 / 100))
            )
        );
        string memory json = string(abi.encodePacked(s, ',"name":"', name, '","cases":[', cases, "]}"));
        vm.writeFile(string(abi.encodePacked("../engine/fixtures/", name, ".json")), json);
    }

    function test_wideSingle() public {
        IUniswapV3Pool pool = _pool(3000, -224000);
        _mint(pool, -240000, -210000, 5e15);
        _run("wide_single", pool);
    }

    function test_stackedNarrow() public {
        IUniswapV3Pool pool = _pool(500, -224003);
        _mint(pool, -224500, -223500, 3e16);
        _mint(pool, -224100, -223900, 8e16);
        _mint(pool, -225000, -224600, 2e16); // below a gap
        _mint(pool, -223400, -222000, 2e16); // above a gap
        _mint(pool, -230000, -218000, 1e15);
        _run("stacked_narrow", pool);
    }

    function test_sparseMultiword() public {
        IUniswapV3Pool pool = _pool(100, -224137);
        _mint(pool, -224700, -223600, 1e16);
        _mint(pool, -224150, -224120, 5e17);
        _mint(pool, -226000, -225100, 3e16);
        _mint(pool, -223000, -221500, 3e16);
        _run("sparse_multiword", pool);
    }

    function test_positivePrice() public {
        // Stock as the cheap side of the pair: exercises positive ticks.
        IUniswapV3Pool pool = _pool(3000, 69000);
        _mint(pool, 60000, 78000, 1e18);
        _mint(pool, 68400, 69600, 4e18);
        _run("positive_price", pool);
    }

    function test_tickMath() public {
        int24[16] memory ts = [int24(-887272), -887271, -500000, -224137, -100, -1, 0, 1, 2, 60, 1000, 69000, 224000, 500000, 887271, 887272];
        string memory out = "";
        for (uint256 i; i < ts.length; ++i) {
            out = string(
                abi.encodePacked(
                    out, i == 0 ? "" : ",", '{"tick":', vm.toString(int256(ts[i])), ',"sqrt":"', vm.toString(uint256(TickMath.getSqrtRatioAtTick(ts[i]))), '"}'
                )
            );
        }
        vm.writeFile("../engine/fixtures/tick_math.json", string(abi.encodePacked("[", out, "]")));
    }
}
