import { useMemo, useState } from 'react'
import snapshotJson from '../public/snapshot.json'
import type { Snapshot, SnapStock } from './types'
import { compact, lltv, pct, ratio, shortAddr, units, usdg } from './format'
import { EXPLORER, PUBLIC_RPC, REPO, sellProceedsTotal, describeError } from './chain'
import type { Address } from 'viem'

const snap = snapshotJson as unknown as Snapshot

type LiveState =
  | { status: 'idle' }
  | { status: 'loading' }
  | { status: 'done'; total: bigint }
  | { status: 'error'; message: string }

/// Depth to compare holdings against. On a composite file the holdings and this figure come from
/// the same run, so the comparison is at one block. The table's sellable columns are a later run
/// and are labelled separately, because comparing across blocks would overstate the gap.
function comparableDepth(s: SnapStock): string {
  return (s.sellable_at_lent_block ?? s.sellable)[0] ?? '0'
}

/// True when more of this stock is pledged to lenders than could be sold at the tightest bound.
function isFlagged(s: SnapStock): boolean {
  if (s.lent_usdg === null) return false
  return BigInt(s.lent_usdg) > BigInt(comparableDepth(s))
}

const sourceFor = (section: string) => snap.sources.find((x) => x.section === section)
const blockOf = (section: string, fallback: number) => sourceFor(section)?.block ?? fallback
const atBlock = (n: number) => `at block ${n.toLocaleString('en-US')}`

function Addr({ address, label }: { address: string; label?: string }) {
  return (
    <a className="addr" href={`${EXPLORER}/address/${address}`} target="_blank" rel="noreferrer">
      {label ?? shortAddr(address)}
    </a>
  )
}

export default function App() {
  const [live, setLive] = useState<Record<string, LiveState>>({})
  const [busy, setBusy] = useState<string | null>(null)

  const aapl = useMemo(() => snap.stocks.find((s) => s.symbol === 'AAPL'), [])
  const flaggedCount = useMemo(() => snap.stocks.filter(isFlagged).length, [])

  async function check(s: SnapStock) {
    if (busy) return
    setBusy(s.symbol)
    setLive((v) => ({ ...v, [s.symbol]: { status: 'loading' } }))
    try {
      const pools = s.pools.map((p) => ({ pool: p.address as Address, stockIsToken0: p.stock_is_token0 }))
      const { total } = await sellProceedsTotal(snap.engine as Address, pools, snap.impacts_bps[0])
      setLive((v) => ({ ...v, [s.symbol]: { status: 'done', total } }))
    } catch (e) {
      setLive((v) => ({ ...v, [s.symbol]: { status: 'error', message: describeError(e) } }))
    } finally {
      setBusy(null)
    }
  }

  const tightest = snap.impacts_bps[0]

  return (
    <div className="page">
      <header>
        <div className="brand">
          <h1>Exitline</h1>
          <p className="tagline">
            How much tokenized stock could actually be sold in a crash, measured onchain, so lending
            cannot quietly outgrow the market that has to absorb it.
          </p>
        </div>
        <div className="snapbadge">
          <span className="label">{snap.composite ? 'Measured snapshot' : 'Snapshot'}</span>
          <span className="block">
            {snap.composite ? 'depth at ' : ''}block {snap.block.toLocaleString('en-US')}
          </span>
          <span className="when">{snap.taken_at_utc}</span>
          <span className="chain">Robinhood Chain, id {snap.chain_id}</span>
          {snap.composite && (
            <span className="chain">
              {snap.sources.length} sources, each at its own block
            </span>
          )}
        </div>
      </header>

      <section className="headline">
        <div className="card">
          <h2>Stock collateral in lending</h2>
          <p className="big">
            {compact(snap.totals.lent_usdg, snap.usdg_decimals)} <span className="unit">USDG</span>
          </p>
          <p className="sub">
            {usdg(snap.totals.lent_usdg, snap.usdg_decimals)} USDG of the six Robinhood equity tokens,
            held across {snap.totals.holders_checked.toLocaleString('en-US')} contracts checked in{' '}
            {snap.lenders.protocols.length} lending protocols,{' '}
            {atBlock(blockOf('lenders', snap.block))}.
          </p>
        </div>

        {aapl && (
          <div className={`card ${isFlagged(aapl) ? 'flagged' : ''}`}>
            <h2>The AAPL gap</h2>
            <p className="big">
              {compact(aapl.lent_usdg, snap.usdg_decimals)}{' '}
              <span className="vs">pledged vs</span>{' '}
              {compact(comparableDepth(aapl), snap.usdg_decimals)}{' '}
              <span className="unit">sellable</span>
            </p>
            <p className="sub">
              {usdg(aapl.lent_usdg, snap.usdg_decimals)} USDG of AAPL sits with lenders, while only{' '}
              {usdg(comparableDepth(aapl), snap.usdg_decimals)} USDG of it could be sold into Uniswap
              V3 before the price falls {pct(tightest)}. Both figures are from the same run,{' '}
              {atBlock(blockOf('lenders', snap.block))}.
              {isFlagged(aapl) ? (
                <>
                  {' '}
                  That is a shortfall of{' '}
                  <strong>
                    {usdg(
                      (BigInt(aapl.lent_usdg ?? '0') - BigInt(comparableDepth(aapl))).toString(),
                      snap.usdg_decimals,
                    )}{' '}
                    USDG
                  </strong>
                  . Liquidators would not get the quoted price, and the difference lands on lenders.
                </>
              ) : (
                ' Depth covered it at that block.'
              )}
            </p>
          </div>
        )}
      </section>

      <section>
        <h2 className="section-title">Every stock, lent against depth</h2>
        <p className="note">
          Lent is what lending contracts hold, valued at the deepest pool's spot price. Sellable is
          the proceeds a seller would receive before the price falls by each bound, walked across
          every Uniswap V3 pool against USDG. A row is flagged when lent exceeds the depth measured
          in the same run, at {pct(tightest)}.
          {flaggedCount > 0 && ` ${flaggedCount} of ${snap.stocks.length} are flagged.`}
          {snap.composite && (
            <>
              {' '}
              Lent is {atBlock(blockOf('lenders', snap.block))} and the sellable columns are{' '}
              {atBlock(blockOf('depth', snap.block))}, so read them as two measurements rather than
              one. Check live gives the current figure.
            </>
          )}
        </p>

        <div className="tablewrap">
          <table>
            <thead>
              <tr>
                <th>Stock</th>
                <th className="num">
                  Lent
                  {snap.composite && <span className="colblock">blk {blockOf('lenders', snap.block)}</span>}
                </th>
                {snap.impacts_bps.map((b) => (
                  <th className="num" key={b}>
                    Sellable {pct(b)}
                    {snap.composite && <span className="colblock">blk {blockOf('depth', snap.block)}</span>}
                  </th>
                ))}
                <th className="num">
                  Lent / depth
                  <span className="colblock">same block</span>
                </th>
                <th>Live check</th>
              </tr>
            </thead>
            <tbody>
              {snap.stocks.map((s) => {
                const r = ratio(s.lent_usdg, comparableDepth(s))
                const st = live[s.symbol] ?? { status: 'idle' }
                return (
                  <tr key={s.symbol} className={isFlagged(s) ? 'flagged' : ''}>
                    <td>
                      <span className="sym">{s.symbol}</span>
                      {isFlagged(s) && <span className="pill">over depth</span>}
                      <br />
                      <Addr address={s.address} />
                    </td>
                    <td className="num">{usdg(s.lent_usdg, snap.usdg_decimals)}</td>
                    {s.sellable.map((v, i) => (
                      <td className="num" key={i}>
                        {usdg(v, snap.usdg_decimals)}
                      </td>
                    ))}
                    <td className="num">
                      {r === null ? 'no depth' : r.toFixed(2) + 'x'}
                    </td>
                    <td className="live">
                      <button onClick={() => check(s)} disabled={busy !== null}>
                        {st.status === 'loading' ? 'Checking...' : 'Check live'}
                      </button>
                      {st.status === 'done' && (
                        <span className="liveval">
                          {usdg(st.total.toString(), snap.usdg_decimals)} USDG now
                          <br />
                          <span className="dim">
                            snapshot {usdg(s.sellable[0], snap.usdg_decimals)}
                          </span>
                        </span>
                      )}
                      {st.status === 'error' && <span className="liveerr">{st.message}</span>}
                    </td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        </div>
        <p className="note">
          Check live calls the deployed engine's <code>sellProceeds</code> at {pct(tightest)} on each
          of that stock's pools, from your browser, through the public endpoint{' '}
          <code>{PUBLIC_RPC}</code>. One stock at a time, on click only.
        </p>
      </section>

      <section>
        <h2 className="section-title">Markets and the vaults behind them</h2>
        <p className="note">
          Canonical Morpho Blue markets with each stock as collateral, and the vaults supplying them.
          Borrowers are a count. No individual is named anywhere on this page or in its data.
        </p>
        {snap.stocks.map((s) => {
          const real = s.markets.filter((m) => BigInt(m.total_supplied) > 0n)
          if (real.length === 0) return null
          return (
            <details key={s.symbol}>
              <summary>
                {s.symbol}: {real.length} market{real.length === 1 ? '' : 's'} with supply, of{' '}
                {s.markets.length} with {s.symbol} collateral
              </summary>
              <div className="tablewrap">
                <table className="sub-table">
                  <thead>
                    <tr>
                      <th>Market</th>
                      <th className="num">LLTV</th>
                      <th className="num">Supplied</th>
                      <th className="num">Borrowed</th>
                      <th className="num">Collateral</th>
                      <th className="num">Borrowers</th>
                      <th>Vaults supplying</th>
                    </tr>
                  </thead>
                  <tbody>
                    {real.map((m) => (
                      <tr key={m.id}>
                        <td>
                          <code className="mid">{m.id.slice(0, 10)}...</code>
                        </td>
                        <td className="num">{lltv(m.lltv_wad)}</td>
                        <td className="num">
                          {usdg(m.total_supplied, m.loan_decimals)} {m.loan_symbol}
                        </td>
                        <td className="num">
                          {usdg(m.total_borrowed, m.loan_decimals)} {m.loan_symbol}
                        </td>
                        <td className="num">
                          {units(m.collateral_units, s.decimals, 2)} {s.symbol}
                        </td>
                        <td className="num">{m.borrower_count}</td>
                        <td>
                          {m.vaults.length === 0 ? (
                            <span className="dim">none registered</span>
                          ) : (
                            <ul className="vaults">
                              {m.vaults.map((v) => (
                                <li key={v.address}>
                                  <Addr address={v.address} label={v.name || shortAddr(v.address)} />{' '}
                                  <span className="dim">
                                    {usdg(v.allocation, m.loan_decimals)} {m.loan_symbol}
                                  </span>
                                </li>
                              ))}
                            </ul>
                          )}
                          {BigInt(m.other_supplied) > 0n && (
                            <span className="dim">
                              {' '}
                              plus {usdg(m.other_supplied, m.loan_decimals)} {m.loan_symbol} from
                              unregistered suppliers
                            </span>
                          )}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </details>
          )
        })}
      </section>

      <section>
        <h2 className="section-title">The guard</h2>
        <p className="note">
          Exitline installs as a sentinel on a Morpho Vault V2 vault. Vault V2 lets a sentinel lower
          supply caps immediately, while every raise goes through the curator's timelock. That
          asymmetry is the design.
        </p>

        <div className="cards">
          <div className="card">
            <h3>What the fork simulation showed</h3>
            <p className="sub">
              Run against the real NetNet Credit Vault V2 on a mainnet fork, with the vault owner
              impersonated on the fork only, fed the AAPL depth measured at the same block.
            </p>
            <table className="mini">
              <tbody>
                <tr>
                  <td>AAPL cap before</td>
                  <td className="num">600,000 USDG</td>
                </tr>
                <tr>
                  <td>AAPL cap after five readings</td>
                  <td className="num">61,753 USDG</td>
                </tr>
                <tr>
                  <td>Depth within 5% at that block</td>
                  <td className="num">123,507 USDG</td>
                </tr>
                <tr>
                  <td>Vault's existing AAPL allocation</td>
                  <td className="num">197,160 USDG</td>
                </tr>
              </tbody>
            </table>
            <p className="sub">
              Measured at block 77,762,358. An earlier run at block 75,821,799 cut the same cap to
              68,336 USDG against roughly 137,000 USDG of depth. In both, the new cap sits below the
              allocation already outstanding, so the cut blocks new lending and leaves open positions
              untouched. The guard then could not raise the cap back, and the curator restored it only
              after the three day timelock.
            </p>
            <p className="sub">
              Rerun it yourself: <code>contracts/script/fork-netnet-aapl.sh</code> measures depth and
              runs the fork test at the same block. It forks the current block by default, so the
              exact figures move with the market.
            </p>
          </div>

          <div className="card">
            <h3>Security model</h3>
            <ol className="model">
              <li>
                <strong>Lowers caps only.</strong> Vault V2 gives a sentinel no power to raise a cap.
              </li>
              <li>
                <strong>Never moves funds.</strong> It cannot borrow, withdraw, or reallocate anything.
              </li>
              <li>
                <strong>Median of five readings.</strong> One or two manipulated readings can neither
                trigger a cut nor block one.
              </li>
              <li>
                <strong>Fixed gas budget per call.</strong> A pool that fails failed inside its full
                budget, not because the transaction was starved.
              </li>
            </ol>
            <p className="sub">
              The guard is not deployed on mainnet. Only the engine is live. It is demonstrated by the
              fork simulation against real Vault V2 code.
            </p>
          </div>
        </div>
      </section>

      <footer>
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
        <p className="enginebox">
          Depth engine, an Arbitrum Stylus contract: <code>{snap.engine}</code>
        </p>
        {snap.composite && (
          <div className="provenance">
            <h3>Where each number came from</h3>
            <table className="mini">
              <tbody>
                {snap.sources.map((src) => (
                  <tr key={src.section}>
                    <td>{src.section}</td>
                    <td className="num">block {src.block.toLocaleString('en-US')}</td>
                    <td className="num">{src.taken_at_utc}</td>
                    <td>
                      <code className="mid">tools/measure/results/{src.file}</code>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
        <ul className="notes">
          {snap.notes.slice(0, 40).map((n, i) => (
            <li key={i}>{n}</li>
          ))}
          {snap.lenders.not_covered.map((n, i) => (
            <li key={`nc${i}`}>Not covered: {n}</li>
          ))}
        </ul>
      </footer>
    </div>
  )
}
