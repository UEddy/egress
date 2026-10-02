import { useEffect, useRef, useState } from 'react'
import { describeError, sellProceedsTotal, type LivePool } from '../chain'

type State =
  | { k: 'idle' }
  | { k: 'busy' }
  | { k: 'done'; total: bigint }
  | { k: 'error'; message: string }

/// Calls the deployed engine for one stock and shows the answer beside the snapshot figure.
///
/// The result slot is always in the DOM and always the same size, so nothing on the page moves
/// when an answer arrives: the slot only changes opacity and transform. Errors are an inline
/// message with a retry, never an alert and never a page level loader.
export function LiveCheck({
  engine,
  pools,
  impactBps,
  snapshotValue,
  label,
  format,
  diffPct,
  disabled,
  onBusyChange,
}: {
  engine: string
  pools: LivePool[]
  impactBps: number
  snapshotValue: bigint
  label: string
  format: (v: bigint) => string
  diffPct: (live: bigint, snap: bigint) => number | null
  disabled: boolean
  onBusyChange: (busy: boolean) => void
}) {
  const [state, setState] = useState<State>({ k: 'idle' })
  const abort = useRef<AbortController | null>(null)

  useEffect(() => () => abort.current?.abort(), [])

  async function run() {
    abort.current?.abort()
    const ac = new AbortController()
    abort.current = ac
    setState({ k: 'busy' })
    onBusyChange(true)
    try {
      const total = await sellProceedsTotal(engine, pools, impactBps, ac.signal)
      setState({ k: 'done', total })
    } catch (e) {
      if ((e as Error)?.name === 'AbortError') return
      setState({ k: 'error', message: describeError(e) })
    } finally {
      if (!ac.signal.aborted) onBusyChange(false)
    }
  }

  const busy = state.k === 'busy'
  const d = state.k === 'done' ? diffPct(state.total, snapshotValue) : null

  return (
    <div className="livecell">
      <button
        type="button"
        className="btn"
        onClick={run}
        disabled={disabled && !busy}
        aria-busy={busy}
        aria-label={state.k === 'idle' ? `Check ${label} depth live onchain` : undefined}
      >
        <span className="btn-label">
          {busy ? 'Checking' : state.k === 'error' ? 'Retry' : state.k === 'idle' ? 'Check live' : 'Check again'}
        </span>
        <span className="btn-prog" aria-hidden="true" data-on={busy ? 'yes' : 'no'} />
      </button>

      {/* Height is reserved whatever the state, so an answer cannot shift the row. */}
      <div className="liveslot" role="status" aria-live="polite">
        <div className={`liveinner ${state.k === 'done' || state.k === 'error' ? 'in' : ''}`}>
          {state.k === 'done' && (
            <>
              <span className="livenum">{format(state.total)}</span>
              {d === null ? (
                <span className="badge" aria-hidden="true">
                  live
                </span>
              ) : Math.abs(d) < 0.05 ? (
                <span className="badge" title="Matches the snapshot">
                  <svg viewBox="0 0 12 12" width="11" height="11" aria-hidden="true" focusable="false">
                    <path d="M1.8 6.4l2.6 2.6L10.2 3" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" />
                  </svg>
                  <span className="vh">matches the snapshot</span>
                </span>
              ) : (
                <span className="badge">
                  {d > 0 ? '+' : ''}
                  {d.toFixed(1)}%
                  <span className="vh"> against the snapshot figure</span>
                </span>
              )}
            </>
          )}
          {state.k === 'error' && <span className="liveerr">{state.message}</span>}
        </div>
      </div>
    </div>
  )
}
