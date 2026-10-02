//! Minimal blocking JSON-RPC client. Every call is pinned to one block so all readings in a run
//! describe the same chain state.

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashMap;
use std::thread::sleep;
use std::time::Duration;

use alloy_primitives::{aliases::I24, Address, Bytes, B256, U256};
use alloy_sol_types::{sol, SolCall};
use exitline_engine::walk::PoolReader;
use serde_json::{json, Value};

/// The public endpoint throttles hard under sustained log queries, and its backoff needs to outlast
/// a burst: 8 tries at 250 ms doubling is about 31 s of patience before giving up.
const MAX_TRIES: u32 = 8;
/// Requests per JSON-RPC batch. The public Robinhood endpoint rate limits per request inside a
/// batch; 20 passes, 100 does not.
const BATCH: usize = 20;

pub struct Rpc {
    agent: ureq::Agent,
    url: String,
    /// Endpoint for eth_getLogs only. Alchemy's free tier caps log ranges at 10 blocks, while the
    /// public endpoint is not an archive node, so state and logs may have to come from different
    /// places. Defaults to `url`, which keeps single-endpoint callers unchanged.
    logs_url: String,
    block: String,
    next_id: Cell<u64>,
    pub calls: Cell<u64>,
}

impl Rpc {
    pub fn new(url: &str) -> Self {
        let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(30)).build();
        Rpc {
            agent,
            url: url.to_string(),
            logs_url: url.to_string(),
            block: "latest".into(),
            next_id: Cell::new(1),
            calls: Cell::new(0),
        }
    }

    /// Sends eth_getLogs to a different endpoint than state reads.
    pub fn set_logs_endpoint(&mut self, url: &str) {
        self.logs_url = url.to_string();
    }

    pub fn logs_endpoint(&self) -> &str {
        &self.logs_url
    }

    /// The endpoint's current head, without pinning anything.
    pub fn head(&self) -> Result<u64, String> {
        let v = self.request("eth_blockNumber", json!([]))?;
        let s = v.as_str().ok_or("eth_blockNumber: not a string")?;
        u64::from_str_radix(s.trim_start_matches("0x"), 16).map_err(|e| e.to_string())
    }

    /// A second client on the same endpoint and pinned block, for another thread.
    pub fn fork(&self) -> Rpc {
        Rpc {
            agent: self.agent.clone(),
            url: self.url.clone(),
            logs_url: self.logs_url.clone(),
            block: self.block.clone(),
            next_id: Cell::new(1),
            calls: Cell::new(0),
        }
    }

    fn body(&self, method: &str, params: Value) -> Value {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
    }

    /// Posts `body`, retrying with backoff while the endpoint rate limits or fails transiently.
    /// A batch whose items come back rate limited is retried whole.
    fn post(&self, body: &Value, what: &str) -> Result<Value, String> {
        self.post_to(&self.url, body, what)
    }

    fn post_to(&self, url: &str, body: &Value, what: &str) -> Result<Value, String> {
        let mut wait = Duration::from_millis(250);
        for attempt in 1..=MAX_TRIES {
            self.calls.set(self.calls.get() + 1);
            match self.agent.post(url).send_json(body) {
                Ok(resp) => {
                    let v: Value = resp.into_json().map_err(|e| format!("{what}: bad json: {e}"))?;
                    if !(rate_limited(&v) && attempt < MAX_TRIES) {
                        return Ok(v);
                    }
                }
                // Public endpoints rate limit; back off and retry.
                Err(ureq::Error::Status(code, _)) if (code == 429 || code >= 500) && attempt < MAX_TRIES => {}
                Err(ureq::Error::Transport(_)) if attempt < MAX_TRIES => {}
                Err(e) => return Err(format!("{what}: {e}")),
            }
            sleep(wait);
            wait *= 2;
        }
        unreachable!()
    }

    fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        let v = self.post(&self.body(method, params), method)?;
        unwrap_result(&v, method)
    }

    fn request_logs(&self, params: Value) -> Result<Value, String> {
        let v = self.post_to(&self.logs_url, &self.body("eth_getLogs", params), "eth_getLogs")?;
        unwrap_result(&v, "eth_getLogs")
    }

    /// Sends `reqs` as JSON-RPC batches and returns one result per request, in order.
    fn request_batch(&self, reqs: Vec<(&str, Value)>) -> Result<Vec<Result<Value, String>>, String> {
        let mut out = Vec::with_capacity(reqs.len());
        for chunk in reqs.chunks(BATCH) {
            let bodies: Vec<Value> = chunk.iter().map(|(m, p)| self.body(m, p.clone())).collect();
            let first = bodies[0]["id"].as_u64().unwrap();
            let v = self.post(&Value::Array(bodies), "batch")?;
            let items = v.as_array().ok_or_else(|| format!("batch: expected an array, got {v}"))?;
            let mut slots: Vec<Option<Result<Value, String>>> = vec![None; chunk.len()];
            for item in items {
                let i = item["id"].as_u64().and_then(|id| id.checked_sub(first)).map(|i| i as usize);
                match i {
                    Some(i) if i < chunk.len() => slots[i] = Some(unwrap_result(item, chunk[i].0)),
                    _ => return Err(format!("batch: unexpected id in {item}")),
                }
            }
            for (i, slot) in slots.into_iter().enumerate() {
                out.push(slot.ok_or_else(|| format!("batch: no response for {}", chunk[i].0))?);
            }
        }
        Ok(out)
    }

    /// Batched eth_calls of one function. Each result is separate: a revert fails only its item.
    pub fn call_many<C: SolCall>(&self, calls: Vec<(Address, C)>) -> Result<Vec<Result<C::Return, String>>, String> {
        let reqs = calls
            .iter()
            .map(|(to, c)| ("eth_call", json!([{ "to": to.to_string(), "data": Bytes::from(c.abi_encode()).to_string() }, self.block])))
            .collect();
        let res = self.request_batch(reqs)?;
        Ok(res
            .into_iter()
            .zip(&calls)
            .map(|(r, (to, _))| {
                let s = r?;
                let b = s.as_str().ok_or("eth_call: result is not a string")?.parse::<Bytes>().map_err(|e| e.to_string())?;
                C::abi_decode_returns(&b).map_err(|e| format!("{} at {to}: {e}", C::SIGNATURE))
            })
            .collect())
    }

    /// Code size at each address, batched.
    pub fn code_sizes(&self, addrs: &[Address]) -> Result<Vec<usize>, String> {
        let reqs = addrs.iter().map(|a| ("eth_getCode", json!([a.to_string(), self.block]))).collect();
        self.request_batch(reqs)?
            .into_iter()
            .map(|r| Ok(r?.as_str().ok_or("eth_getCode: not a string")?.len().saturating_sub(2) / 2))
            .collect()
    }

    /// Logs from `address` whose topic `index` (1..=3) equals `value`, from `from` up to the pinned
    /// block.
    pub fn logs_by_topic(&self, address: Address, index: usize, value: B256, from: u64) -> Result<Vec<Value>, String> {
        let mut topics = vec![None; index + 1];
        topics[index] = Some(value);
        self.logs(address, &topics, from)
    }

    /// Logs from `address` matching `topics` (None matches anything), from `from` up to the pinned
    /// block. Ranges that exceed the endpoint's result or time limits are split in half.
    pub fn logs(&self, address: Address, topics: &[Option<B256>], from: u64) -> Result<Vec<Value>, String> {
        let topics: Vec<Value> = topics.iter().map(|t| t.map_or(Value::Null, |t| Value::String(t.to_string()))).collect();
        let to = u64::from_str_radix(self.block.trim_start_matches("0x"), 16).map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        let mut stack = vec![(from, to)];
        while let Some((a, b)) = stack.pop() {
            let params = json!([{
                "address": address.to_string(),
                "topics": topics,
                "fromBlock": format!("0x{a:x}"),
                "toBlock": format!("0x{b:x}"),
            }]);
            match self.request_logs(params) {
                Ok(Value::Array(logs)) => out.extend(logs),
                Ok(v) => return Err(format!("eth_getLogs: unexpected {v}")),
                // Too many results, too slow, or (since Sep 29) a range over 10M blocks.
                Err(e) if b > a && (e.contains("exceeds limit") || e.contains("timed out") || e.contains("narrow the block range")) => {
                    let m = a + (b - a) / 2;
                    stack.push((m + 1, b));
                    stack.push((a, m));
                }
                Err(e) => return Err(e),
            }
        }
        Ok(out)
    }

    pub fn chain_id(&self) -> Result<u64, String> {
        hex_u64(&self.request("eth_chainId", json!([]))?)
    }

    /// Pins all later calls to `block` (or the latest block) and returns (number, timestamp).
    pub fn pin_block(&mut self, block: Option<u64>) -> Result<(u64, u64), String> {
        let tag = block.map(|b| format!("0x{b:x}")).unwrap_or_else(|| "latest".into());
        let b = self.request("eth_getBlockByNumber", json!([tag, false]))?;
        if b.is_null() {
            return Err(format!("block {tag} not found"));
        }
        let number = hex_u64(&b["number"])?;
        let timestamp = hex_u64(&b["timestamp"])?;
        self.block = format!("0x{number:x}");
        Ok((number, timestamp))
    }

    pub fn call_raw(&self, to: Address, data: Vec<u8>) -> Result<Bytes, String> {
        self.eth_call(json!([{ "to": to.to_string(), "data": Bytes::from(data).to_string() }, self.block]))
    }

    /// eth_call with `code` placed at `to` through a state override.
    pub fn call_with_code<C: SolCall>(&self, to: Address, code: &str, call: C) -> Result<C::Return, String> {
        let out = self.eth_call(json!([
            { "to": to.to_string(), "data": Bytes::from(call.abi_encode()).to_string() },
            self.block,
            { to.to_string(): { "code": code } }
        ]))?;
        C::abi_decode_returns(&out).map_err(|e| format!("{} at {to}: {e}", C::SIGNATURE))
    }

    fn eth_call(&self, params: Value) -> Result<Bytes, String> {
        let r = self.request("eth_call", params)?;
        let s = r.as_str().ok_or("eth_call: result is not a string")?;
        s.parse::<Bytes>().map_err(|e| format!("eth_call: {e}"))
    }

    pub fn call<C: SolCall>(&self, to: Address, call: C) -> Result<C::Return, String> {
        let out = self.call_raw(to, call.abi_encode())?;
        C::abi_decode_returns(&out).map_err(|e| format!("{} at {to}: {e}", C::SIGNATURE))
    }
}

fn rate_limited(v: &Value) -> bool {
    let is = |x: &Value| x["error"]["code"].as_i64() == Some(429);
    match v {
        Value::Array(items) => items.iter().any(is),
        other => is(other),
    }
}

fn unwrap_result(v: &Value, what: &str) -> Result<Value, String> {
    if let Some(err) = v.get("error") {
        if err.to_string().contains("historical state") {
            return Err(format!(
                "{what}: {err}\nThe RPC pruned the pinned block's state mid-run (the public endpoint keeps \
                 roughly 10 minutes). Rerun, or pass an archive endpoint with --rpc."
            ));
        }
        return Err(format!("{what}: {err}"));
    }
    v.get("result").cloned().ok_or_else(|| format!("{what}: no result"))
}

fn hex_u64(v: &Value) -> Result<u64, String> {
    let s = v.as_str().ok_or("expected hex string")?;
    u64::from_str_radix(s.trim_start_matches("0x"), 16).map_err(|e| e.to_string())
}

sol! {
    interface IUniswapV3Pool {
        function slot0() external view returns (uint160 sqrtPriceX96, int24 tick, uint16 observationIndex, uint16 observationCardinality, uint16 observationCardinalityNext, uint8 feeProtocol, bool unlocked);
        function liquidity() external view returns (uint128);
        function tickSpacing() external view returns (int24);
        function tickBitmap(int16 word) external view returns (uint256);
        function ticks(int24 tick) external view returns (uint128 liquidityGross, int128 liquidityNet, uint256 feeGrowthOutside0X128, uint256 feeGrowthOutside1X128, int56 tickCumulativeOutside, uint160 secondsPerLiquidityOutsideX128, uint32 secondsOutside, bool initialized);
        function token0() external view returns (address);
        function token1() external view returns (address);
        function factory() external view returns (address);
        function fee() external view returns (uint24);
    }

    interface IUniswapV3Factory {
        function getPool(address a, address b, uint24 fee) external view returns (address);
    }

    interface IERC20 {
        function symbol() external view returns (string);
        function decimals() external view returns (uint8);
        function balanceOf(address who) external view returns (uint256);
    }

    interface IMetaMorpho {
        function MORPHO() external view returns (address);
        function asset() external view returns (address);
        function supplyQueueLength() external view returns (uint256);
        function supplyQueue(uint256 i) external view returns (bytes32);
        function withdrawQueueLength() external view returns (uint256);
        function withdrawQueue(uint256 i) external view returns (bytes32);
    }

    struct MarketParams {
        address loanToken;
        address collateralToken;
        address oracle;
        address irm;
        uint256 lltv;
    }

    struct Market {
        uint128 totalSupplyAssets;
        uint128 totalSupplyShares;
        uint128 totalBorrowAssets;
        uint128 totalBorrowShares;
        uint128 lastUpdate;
        uint128 fee;
    }

    interface IMorpho {
        function idToMarketParams(bytes32 id) external view returns (MarketParams memory);
        function market(bytes32 id) external view returns (Market memory);
    }

    interface IIrm {
        function borrowRateView(MarketParams memory marketParams, Market memory market) external view returns (uint256);
    }

    interface IVaultConfig {
        function config(bytes32 id) external view returns (uint184 cap, bool enabled, uint64 removableAt);
    }

    interface ISwapProbe {
        function probe(address pool, bool zeroForOne, uint160 limit) external returns (int256 amount0, int256 amount1);
    }

    struct GageLoan {
        address originator;
        address account;
        address token;
        address collateralBeneficiary;
        uint8 kind;
        uint8 state;
        uint8 filled;
        uint32 term;
        uint40 fundingDeadline;
        uint40 fundedAt;
        uint40 closedAt;
        uint128 principal;
        uint128 cap;
        uint128 originationFee;
        uint128 borrowerReward;
        uint128 lenderReward;
        uint256 collateral;
        bytes32 exposureKey;
        uint256 exposureAmount;
    }

    interface ILenders {
        function getReserveTokensAddresses(address asset) external view returns (address aTokenAddress, address stableDebtTokenAddress, address variableDebtTokenAddress);
        function getAddr(uint256 regId) external view returns (address);
        function loanCount() external view returns (uint256);
        function getLoan(uint256 id) external view returns (GageLoan memory);
        function nextOfferId() external view returns (uint256);
        function vaults(uint256 id) external view returns (address);
        function allAccountsLength() external view returns (uint256);
        function allAccounts(uint256 i) external view returns (address);
        function tokens() external view returns (address fixedToken, address xToken, address gearingToken, address collateral, address debt);
    }

    interface IOracle {
        function price() external view returns (uint256);
    }
}

/// Pool state for the engine's walk, read over RPC at the pinned block. Reads are memoized
/// because the walk is repeated for several impact bounds on the same pool.
pub struct RpcPool<'a> {
    rpc: &'a Rpc,
    pub pool: Address,
    slot0: OnceCell<(U256, i32)>,
    liquidity: OnceCell<u128>,
    spacing: OnceCell<i32>,
    words: RefCell<HashMap<i16, U256>>,
    nets: RefCell<HashMap<i32, i128>>,
}

impl<'a> RpcPool<'a> {
    pub fn new(rpc: &'a Rpc, pool: Address) -> Self {
        RpcPool {
            rpc,
            pool,
            slot0: OnceCell::new(),
            liquidity: OnceCell::new(),
            spacing: OnceCell::new(),
            words: Default::default(),
            nets: Default::default(),
        }
    }
}

/// Bitmap words fetched per prefetch at most. Wider walks fall back to reads on demand.
const PREFETCH_MAX_WORDS: i32 = 512;

impl RpcPool<'_> {
    /// Warms the caches with every bitmap word and initialized tick a sell of up to `max_bps`
    /// impact can touch, in two batched requests. The walk itself is unchanged: it reads through
    /// the same cache, and anything outside the prefetched range is read on demand.
    pub fn prefetch(&self, stock_is_token0: bool, max_bps: u64) -> Result<(), String> {
        use exitline_engine::math::{compress, word_position, MAX_TICK, MIN_TICK};
        let (_, tick) = self.slot0()?;
        let spacing = self.tick_spacing()?;
        self.liquidity()?;
        if spacing <= 0 || max_bps == 0 || max_bps >= 10_000 {
            return Ok(());
        }
        // Ticks the price moves for this impact, plus a margin of two spacings.
        let ratio = 1.0 - max_bps as f64 / 10_000.0;
        let delta = ((1.0 / ratio).ln() / 1.0001f64.ln()).ceil() as i32 + 2 * spacing;
        let (lo, hi) = if stock_is_token0 {
            ((tick - delta).max(MIN_TICK), tick)
        } else {
            (tick, (tick + delta).min(MAX_TICK))
        };
        let w_lo = word_position(compress(lo, spacing)).0 as i32 - 1;
        let w_hi = word_position(compress(hi, spacing)).0 as i32 + 1;
        if w_hi - w_lo + 1 > PREFETCH_MAX_WORDS {
            return Ok(());
        }
        let words: Vec<i16> = (w_lo..=w_hi).filter_map(|w| i16::try_from(w).ok()).collect();
        let calls = words.iter().map(|&w| (self.pool, IUniswapV3Pool::tickBitmapCall { word: w })).collect();
        let mut ticks = Vec::new();
        for (&w, r) in words.iter().zip(self.rpc.call_many(calls)?) {
            let bits = r?;
            self.words.borrow_mut().insert(w, bits);
            for b in 0..256usize {
                if bits.bit(b) {
                    let t = ((w as i32) * 256 + b as i32) * spacing;
                    if t >= lo - spacing && t <= hi + spacing {
                        ticks.push(t);
                    }
                }
            }
        }
        let calls = ticks
            .iter()
            .map(|&t| Ok((self.pool, IUniswapV3Pool::ticksCall { tick: I24::try_from(t).map_err(|_| format!("tick {t}"))? })))
            .collect::<Result<_, String>>()?;
        for (&t, r) in ticks.iter().zip(self.rpc.call_many(calls)?) {
            self.nets.borrow_mut().insert(t, r?.liquidityNet);
        }
        Ok(())
    }
}

impl PoolReader for RpcPool<'_> {
    type Error = String;

    fn slot0(&self) -> Result<(U256, i32), String> {
        if let Some(v) = self.slot0.get() {
            return Ok(*v);
        }
        let r = self.rpc.call(self.pool, IUniswapV3Pool::slot0Call {})?;
        Ok(*self.slot0.get_or_init(|| (U256::from(r.sqrtPriceX96), r.tick.as_i32())))
    }

    fn liquidity(&self) -> Result<u128, String> {
        if let Some(v) = self.liquidity.get() {
            return Ok(*v);
        }
        let l = self.rpc.call(self.pool, IUniswapV3Pool::liquidityCall {})?;
        Ok(*self.liquidity.get_or_init(|| l))
    }

    fn tick_spacing(&self) -> Result<i32, String> {
        if let Some(v) = self.spacing.get() {
            return Ok(*v);
        }
        let t = self.rpc.call(self.pool, IUniswapV3Pool::tickSpacingCall {})?.as_i32();
        Ok(*self.spacing.get_or_init(|| t))
    }

    fn tick_bitmap(&self, word: i16) -> Result<U256, String> {
        if let Some(w) = self.words.borrow().get(&word) {
            return Ok(*w);
        }
        let w = self.rpc.call(self.pool, IUniswapV3Pool::tickBitmapCall { word })?;
        self.words.borrow_mut().insert(word, w);
        Ok(w)
    }

    fn liquidity_net(&self, tick: i32) -> Result<i128, String> {
        if let Some(n) = self.nets.borrow().get(&tick) {
            return Ok(*n);
        }
        let t = I24::try_from(tick).map_err(|_| format!("tick {tick} out of int24"))?;
        let n = self.rpc.call(self.pool, IUniswapV3Pool::ticksCall { tick: t })?.liquidityNet;
        self.nets.borrow_mut().insert(tick, n);
        Ok(n)
    }
}
