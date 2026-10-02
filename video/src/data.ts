// Everything the video says that is a number comes from here, and everything here comes from a
// file produced by a measurement run. Nothing on screen is typed by hand.
//
//   ../web/public/snapshot.json  the dashboard's committed data, imported at build time
//   ./data/fork-result.json      parsed from a real run of contracts/script/fork-netnet-aapl.sh
import snapshot from '../../web/public/snapshot.json'
import crosscheck from '../../tools/measure/results/engine-crosscheck-77749579.json'
import fork from './data/fork-result.json'

type Stock = (typeof snapshot.stocks)[number]

export const snap = snapshot
export const forkRun = fork

const DEC = snapshot.usdg_decimals

/** An integer string in token units to a float, for display only. */
export const toFloat = (v: string | null | undefined, decimals = DEC): number => {
  if (!v) return 0
  const n = BigInt(v)
  const scale = 10n ** BigInt(decimals)
  return Number(n / scale) + Number(n % scale) / Number(scale)
}

export const fmt = (n: number, max = 0): string =>
  n.toLocaleString('en-US', { minimumFractionDigits: 0, maximumFractionDigits: max })

export const fmtUsdg = (v: string | null | undefined): string => fmt(toFloat(v))

/** The block a given section of the snapshot was measured at. */
export const blockOf = (section: string): number =>
  snapshot.sources?.find((s) => s.section === section)?.block ?? snapshot.block

export const stock = (symbol: string): Stock => {
  const s = snapshot.stocks.find((x) => x.symbol === symbol)
  if (!s) throw new Error(`snapshot.json has no stock ${symbol}`)
  return s
}

/**
 * Depth measured in the SAME run as the holdings, so the two can be compared at one block.
 * `sellable` is from the newer depth run and is not comparable with `lent_usdg` directly.
 */
export const comparableDepth = (s: Stock): string =>
  (s.sellable_at_lent_block ?? s.sellable)[0] ?? '0'

const aaplStock = stock('AAPL')

export const AAPL = {
  lent: aaplStock.lent_usdg ?? '0',
  sellable: comparableDepth(aaplStock),
  lentF: toFloat(aaplStock.lent_usdg),
  sellableF: toFloat(comparableDepth(aaplStock)),
  gapF: toFloat(aaplStock.lent_usdg) - toFloat(comparableDepth(aaplStock)),
  block: blockOf('lenders'),
}

export const TOTAL = {
  lent: snapshot.totals.lent_usdg,
  lentF: toFloat(snapshot.totals.lent_usdg),
  holders: snapshot.totals.holders_checked,
  protocols: snapshot.lenders.protocols.length,
  block: blockOf('lenders'),
}

/**
 * The verification tally, read from the committed cross-check file rather than recomputed.
 * `egress-measure verify-engine` asked the deployed engine every reading that file records, at its
 * pinned block, and wrote the result back as `engine_verification`. Quoting it from there means the
 * figure on screen is a record of a run, not an assumption about how many readings there ought to be.
 */
export const VERIFY = {
  cases: crosscheck.engine_verification.cases,
  matched: crosscheck.engine_verification.matched,
  mismatched: crosscheck.engine_verification.mismatched,
  block: crosscheck.engine_verification.block,
  engine: crosscheck.engine_verification.engine,
  file: 'tools/measure/results/engine-crosscheck-77749579.json',
}

export const ENGINE = snapshot.engine
export const CHAIN_ID = snapshot.chain_id
export const DEPTH_BLOCK = blockOf('depth')
export const TIGHTEST_BPS = snapshot.impacts_bps[0]
export const TIGHTEST_PCT = `${TIGHTEST_BPS / 100}%`

/** Stocks ordered by how much is lent against them, biggest first. */
export const BY_EXPOSURE = [...snapshot.stocks].sort(
  (a, b) => toFloat(b.lent_usdg) - toFloat(a.lent_usdg),
)

export const FORK = {
  file: 'video/src/data/fork-result.json',
  block: fork.block,
  realDurationSeconds: fork.recording.real_duration_seconds,
  playbackSpeed: fork.recording.playback_speed,
  capBefore: Number(fork.cap_before_usdg),
  capAfter: Number(fork.cap_after_usdg),
  depth: Number(fork.sellable_within_5pct_usdg),
  allocation: Number(fork.allocation_usdg),
  timelockDays: Math.round(fork.timelock_seconds / 86400),
}

export const LINKS = {
  repo: 'github.com/UEddy/egress',
  dashboard: 'egress-theta.vercel.app',
}
