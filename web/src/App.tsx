import { useMemo, useState } from 'react'
import snapshotJson from '../public/snapshot.json'
import type { Snapshot, SnapStock } from './types'
import { compact, lltv, pct, ratio, shortAddr, toFloat, units, usdg } from './format'
import { EXPLORER, PUBLIC_RPC, REPO } from './chain'
import { Reveal } from './components/Reveal'
import { CountUp } from './components/CountUp'
import { LiveCheck } from './components/LiveCheck'

const snap = snapshotJson as unknown as Snapshot
const DEC = snap.usdg_decimals
const TIGHTEST = snap.impacts_bps[0]

/// Depth to compare holdings against. On a composite file the holdings and this figure come from
/// the same run, so the comparison sits at one block. The sellable columns are a later run and are
/// labelled separately, because comparing across blocks would overstate the gap.
function comparableDepth(s: SnapStock): string {
  return (s.sellable_at_lent_block ?? s.sellable)[0] ?? '0'
}

function isFlagged(s: SnapStock): boolean {
  if (s.lent_usdg === null) return false
  return BigInt(s.lent_usdg) > BigInt(comparableDepth(s))
}

const sourceFor = (section: string) => snap.sources?.find((x) => x.section === section)
const blockOf = (section: string) => sourceFor(section)?.block ?? snap.block
const atBlock = (n: number) => `at block ${n.toLocaleString('en-US')}`
const fmtUsdg = (v: bigint) => usdg(v.toString(), DEC) + ' USDG'

function diffPct(live: bigint, snapshot: bigint): number | null {
  if (snapshot === 0n) return null
  return (Number((live * 10000n) / snapshot) / 100) - 100
}

function Addr({ address, label }: { address: string; label?: string }) {
  return (
    <a className="addr" href={`${EXPLORER}/address/${address}`} target="_blank" rel="noreferrer">
      {label ?? shortAddr(address)}
    </a>
  )
}

const NAV = [
  { id: 'overview', label: 'Overview' },
  { id: 'depth', label: 'Depth' },
  { id: 'markets', label: 'Markets' },
  { id: 'guard', label: 'The guard' },
]

export default function App() {
  const [busy, setBusy] = useState(false)
  const aapl = useMemo(() => snap.stocks.find((s) => s.symbol === 'AAPL'), [])
  const flagged = useMemo(() => snap.stocks.filter(isFlagged), [])

  return (
    <>
      <a className="skip" href="#overview">
        Skip to content
      </a>

      <header className="bar">
        <div className="bar-in">
          <a className="mark" href="#top">
            Exitline
          </a>
          <nav aria-label="Sections">
            <ul>
              {NAV.map((n) => (
                <li key={n.id}>
                  <a href={`#${n.id}`}>{n.label}</a>
                </li>
              ))}
            </ul>
          </nav>
          <a className="bar-src" href={REPO} target="_blank" rel="noreferrer">
            GitHub
          </a>
        </div>
      </header>

      <main className="page" id="top">
        <Reveal as="section" className="hero" index={0} id="overview" ariaLabelledby="hero-h">
          <h1 id="hero-h">
            Lending against tokenized stock,
            <br />
            measured against what can actually be sold.
          </h1>
          <p className="lede">
            Exitline walks Uniswap V3 tick by tick, onchain, to find how much of a stock a seller
            could really get out before the price falls. When loans outgrow that, it stops new
            lending before the shortfall lands on lenders.
          </p>
          <dl className="meta">
            <div>
              <dt>Chain</dt>
              <dd>Robinhood Chain, id {snap.chain_id}</dd>
            </div>
            <div>
              <dt>Depth measured</dt>
              <dd>block {blockOf('depth').toLocaleString('en-US')}</dd>
            </div>
            <div>
              <dt>Holdings measured</dt>
              <dd>block {blockOf('lenders').toLocaleString('en-US')}</dd>
            </div>
          </dl>
        </Reveal>

        <div className="headline">
          <Reveal className="card" index={1}>
            <h2>Stock collateral in lending</h2>
            <p className="figure">
              <CountUp value={toFloat(snap.totals.lent_usdg, DEC)} format={(n) => compact(BigInt(Math.round(n * 10 ** DEC)), DEC)} />
              <span className="unit">USDG</span>
            </p>
            <p className="sub">
              {usdg(snap.totals.lent_usdg, DEC)} USDG of the six Robinhood equity tokens, across{' '}
              {snap.totals.holders_checked.toLocaleString('en-US')} contracts in{' '}
              {snap.lenders.protocols.length} protocols, {atBlock(blockOf('lenders'))}.
            </p>
          </Reveal>

          {aapl && (
            <Reveal className={`card ${isFlagged(aapl) ? 'flag' : ''}`} index={2}>
              <h2>The AAPL gap</h2>
              <p className="figure">
                <CountUp value={toFloat(aapl.lent_usdg ?? '0', DEC)} format={(n) => compact(BigInt(Math.round(n * 10 ** DEC)), DEC)} />
                <span className="unit">pledged</span>
                <span className="vs">vs</span>
                <CountUp value={toFloat(comparableDepth(aapl), DEC)} format={(n) => compact(BigInt(Math.round(n * 10 ** DEC)), DEC)} />
                <span className="unit">sellable</span>
              </p>
              <p className="sub">
                {usdg(aapl.lent_usdg, DEC)} USDG of AAPL sits with lenders, while only{' '}
                {usdg(comparableDepth(aapl), DEC)} USDG of it could be sold before the price falls{' '}
                {pct(TIGHTEST)}. Both figures come from the same run, {atBlock(blockOf('lenders'))}.
              </p>
              {isFlagged(aapl) && (
                <p className="sub strong">
                  A shortfall of{' '}
                  {usdg((BigInt(aapl.lent_usdg ?? '0') - BigInt(comparableDepth(aapl))).toString(), DEC)}{' '}
                  USDG. Liquidators would not get the quoted price, and the difference lands on
                  lenders.
                </p>
              )}
            </Reveal>
          )}
        </div>

        <Reveal as="section" index={0} id="depth" ariaLabelledby="depth-h">
          <h2 className="sh" id="depth-h">
            Every stock, lent against depth
          </h2>
          <p className="note">
            Lent is what lending contracts hold, valued at the deepest pool's spot price. Sellable is
            what a seller would receive before the price falls by each bound, walked across every
            Uniswap V3 pool against USDG. A stock is flagged when lent exceeds the depth measured in
            the same run, at {pct(TIGHTEST)}.{' '}
            {flagged.length > 0 && `${flagged.length} of ${snap.stocks.length} is flagged.`} Lent is{' '}
            {atBlock(blockOf('lenders'))} and the sellable figures are {atBlock(blockOf('depth'))}, so
            read them as two measurements. Check live gives the figure right now.
          </p>

          <div className="grid" role="table" aria-label="Stock collateral against sellable depth">
            <div role="rowgroup">
              <div className="row head" role="row">
                <span role="columnheader">Stock</span>
                <span role="columnheader" className="n">
                  Lent
                </span>
                {snap.impacts_bps.map((b) => (
                  <span role="columnheader" className="n" key={b}>
                    Sellable {pct(b)}
                  </span>
                ))}
                <span role="columnheader" className="n">
                  Lent / depth
                  <span className="colsub">same run</span>
                </span>
                <span role="columnheader">Live check</span>
              </div>
            </div>
            <div role="rowgroup">
              {snap.stocks.map((s, i) => {
                const r = ratio(s.lent_usdg, comparableDepth(s))
                const flag = isFlagged(s)
                return (
                  <Reveal as="div" role="row" index={i} className={`row ${flag ? 'flag' : ''}`} key={s.symbol}>
                    <span role="cell" className="c-sym">
                      <span className="sym">{s.symbol}</span>
                      {flag && <span className="pill">over depth</span>}
                      <Addr address={s.address} />
                    </span>
                    <span role="cell" className="n">
                      <span className="lbl">Lent</span>
                      {usdg(s.lent_usdg, DEC)}
                    </span>
                    {s.sellable.map((v, j) => (
                      <span role="cell" className="n" key={j}>
                        <span className="lbl">Sellable {pct(snap.impacts_bps[j])}</span>
                        {usdg(v, DEC)}
                      </span>
                    ))}
                    <span role="cell" className="n">
                      <span className="lbl">Lent / depth, same run</span>
                      {r === null ? 'no depth' : r.toFixed(2) + 'x'}
                    </span>
                    <span role="cell" className="c-live">
                      <LiveCheck
                        engine={snap.engine}
                        pools={s.pools.map((p) => ({ pool: p.address, stockIsToken0: p.stock_is_token0 }))}
                        impactBps={TIGHTEST}
                        snapshotValue={BigInt(s.sellable[0] ?? '0')}
                        label={s.symbol}
                        format={fmtUsdg}
                        diffPct={diffPct}
                        disabled={busy}
                        onBusyChange={setBusy}
                      />
                    </span>
                  </Reveal>
                )
              })}
            </div>
          </div>
          <p className="note small">
            Check live calls the engine's <code>sellProceeds</code> at {pct(TIGHTEST)} on each of that
            stock's pools, from your browser, through the public endpoint <code>{PUBLIC_RPC}</code>.
            One stock at a time, on click only.
          </p>
        </Reveal>

        <Reveal as="section" index={0} id="markets" ariaLabelledby="markets-h">
          <h2 className="sh" id="markets-h">
            Markets and the vaults behind them
          </h2>
          <p className="note">
            Canonical Morpho Blue markets with each stock as collateral, and the vaults supplying
            them, {atBlock(blockOf('markets:AAPL'))}. Borrowers are a count. No individual is named
            on this page or in its data.
          </p>
          {snap.stocks.map((s) => {
            const real = s.markets.filter((m) => BigInt(m.total_supplied) > 0n)
            if (real.length === 0) return null
            return (
              <details key={s.symbol}>
                <summary>
                  <span className="sym">{s.symbol}</span>
                  <span className="dim">
                    {real.length} market{real.length === 1 ? '' : 's'} with supply, of {s.markets.length}{' '}
                    with {s.symbol} collateral
                  </span>
                </summary>
                <ul className="mkts">
                  {real.map((m) => (
                    <li key={m.id}>
                      <div className="mkt-top">
                        <code className="mid">{m.id.slice(0, 14)}...</code>
                        <span className="dim">LLTV {lltv(m.lltv_wad)}</span>
                      </div>
                      <dl className="mkt-stats">
                        <div>
                          <dt>Supplied</dt>
                          <dd>
                            {usdg(m.total_supplied, m.loan_decimals)} {m.loan_symbol}
                          </dd>
                        </div>
                        <div>
                          <dt>Borrowed</dt>
                          <dd>
                            {usdg(m.total_borrowed, m.loan_decimals)} {m.loan_symbol}
                          </dd>
                        </div>
                        <div>
                          <dt>Collateral</dt>
                          <dd>
                            {units(m.collateral_units, s.decimals, 2)} {s.symbol}
                          </dd>
                        </div>
                        <div>
                          <dt>Borrowers</dt>
                          <dd>{m.borrower_count}</dd>
                        </div>
                      </dl>
                      {m.vaults.length > 0 && (
                        <ul className="vaults">
                          {m.vaults.map((v) => (
                            <li key={v.address}>
                              <Addr address={v.address} label={v.name || shortAddr(v.address)} />
                              <span className="dim">
                                {usdg(v.allocation, m.loan_decimals)} {m.loan_symbol}
                              </span>
                            </li>
                          ))}
                        </ul>
                      )}
                      {BigInt(m.other_supplied) > 0n && (
                        <p className="dim small">
                          plus {usdg(m.other_supplied, m.loan_decimals)} {m.loan_symbol} from
                          suppliers that are not registered vaults
                        </p>
                      )}
                    </li>
                  ))}
                </ul>
              </details>
            )
          })}
        </Reveal>

        <Reveal as="section" index={0} id="guard" ariaLabelledby="guard-h">
          <h2 className="sh" id="guard-h">
            The guard
          </h2>
          <p className="note">
            Exitline installs as a sentinel on a Morpho Vault V2 vault. Vault V2 lets a sentinel
            lower supply caps immediately, while every raise goes through the curator's timelock.
            That asymmetry is the design.
          </p>
          <div className="headline">
            <Reveal className="card" index={1}>
              <h3>What the fork simulation showed</h3>
              <dl className="mkt-stats wide">
                <div>
                  <dt>AAPL cap before</dt>
                  <dd>600,000 USDG</dd>
                </div>
                <div>
                  <dt>Cap after five readings</dt>
                  <dd className="accent">61,753 USDG</dd>
                </div>
                <div>
                  <dt>Depth within 5%</dt>
                  <dd>123,507 USDG</dd>
                </div>
                <div>
                  <dt>Existing allocation</dt>
                  <dd>197,160 USDG</dd>
                </div>
              </dl>
              <p className="sub">
                Run against the real NetNet Credit Vault V2 on a mainnet fork at block 77,762,358,
                with the vault owner impersonated on the fork only, fed the AAPL depth measured at
                that block. An earlier run at block 75,821,799 cut the same cap to 68,336 USDG
                against roughly 137,000 USDG of depth.
              </p>
              <p className="sub">
                The new cap sits below the allocation already outstanding, so the cut blocks new
                lending and leaves open positions untouched. The guard then could not raise the cap
                back, and the curator restored it only after the three day timelock.
              </p>
              <p className="sub">
                Rerun it: <code>contracts/script/fork-netnet-aapl.sh</code> measures depth and runs
                the fork test at the same block. It forks the current block by default, so the exact
                figures move with the market.
              </p>
            </Reveal>

            <Reveal className="card" index={2}>
              <h3>Security model</h3>
              <ol className="model">
                <li>
                  <strong>Lowers caps only.</strong> Vault V2 gives a sentinel no power to raise one.
                </li>
                <li>
                  <strong>Never moves funds.</strong> It cannot borrow, withdraw or reallocate
                  anything.
                </li>
                <li>
                  <strong>Median of five readings.</strong> One or two manipulated readings can
                  neither trigger a cut nor block one.
                </li>
                <li>
                  <strong>Fixed gas budget per call.</strong> A pool that fails failed inside its
                  full budget, not because the transaction was starved.
                </li>
              </ol>
              <p className="sub">
                The guard is not deployed on mainnet. Only the engine is live. It is demonstrated by
                the fork simulation against real Vault V2 code.
              </p>
            </Reveal>
          </div>
        </Reveal>

        <Reveal as="section" index={0} className="foot" ariaLabelledby="foot-h">
          <h2 className="sh" id="foot-h">
            Sources and links
          </h2>
          <div className="links">
            <a href={REPO} target="_blank" rel="noreferrer">
              Source on GitHub
            </a>
            <a href={`${EXPLORER}/address/${snap.engine}`} target="_blank" rel="noreferrer">
              Depth engine on the explorer
            </a>
            <a href={`${EXPLORER}/address/${snap.morpho_blue}`} target="_blank" rel="noreferrer">
              Morpho Blue
            </a>
          </div>
          <p className="note small">
            Depth engine, an Arbitrum Stylus contract: <code>{snap.engine}</code>
          </p>
          {snap.composite && snap.sources && (
            <>
              <h3 className="sh3">Where each number came from</h3>
              <ul className="srcs">
                {snap.sources.map((src) => (
                  <li key={src.section}>
                    <span className="sym">{src.section}</span>
                    <span className="n">block {src.block.toLocaleString('en-US')}</span>
                    <span className="dim">{src.taken_at_utc}</span>
                    <code className="mid">{src.file}</code>
                  </li>
                ))}
              </ul>
            </>
          )}
          <ul className="notes">
            {snap.notes.map((n, i) => (
              <li key={i}>{n}</li>
            ))}
            {snap.lenders.not_covered.map((n, i) => (
              <li key={`nc${i}`}>Not covered: {n}</li>
            ))}
          </ul>
        </Reveal>
      </main>
    </>
  )
}
