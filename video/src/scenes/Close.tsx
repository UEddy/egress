import { interpolate, useCurrentFrame } from 'remotion'
import { C, MONO, tnum } from '../theme'
import { Caption, Rise, Scene, Wordmark } from '../ui'
import { CHAIN_ID, ENGINE, LINKS } from '../data'

const Link: React.FC<{ at: number; label: string; url: string }> = ({ at, label, url }) => {
  const frame = useCurrentFrame()
  return (
    <div
      style={{
        opacity: interpolate(frame, [at, at + 18], [0, 1], { extrapolateLeft: 'clamp', extrapolateRight: 'clamp' }),
        translate: `0px ${interpolate(frame, [at, at + 18], [14, 0], {
          extrapolateLeft: 'clamp',
          extrapolateRight: 'clamp',
          easing: (t) => 1 - Math.pow(1 - t, 3),
        })}px`,
      }}
    >
      <div style={{ fontSize: 22, letterSpacing: '0.14em', textTransform: 'uppercase', color: C.dim, fontWeight: 600 }}>
        {label}
      </div>
      <div style={{ fontFamily: MONO, fontSize: 40, marginTop: 8, color: C.text, ...tnum }}>{url}</div>
    </div>
  )
}

/** 2:10 to 2:30. Where it runs, what it is paid in, and where to find it. */
export const Close: React.FC = () => {
  const frame = useCurrentFrame()
  return (
    <Scene>
      <Rise at={4} dur={22} y={22}>
        <div style={{ fontSize: 80, fontWeight: 650, letterSpacing: '-0.03em', lineHeight: 1.14, maxWidth: 1620 }}>
          Built on Robinhood Chain with <span style={{ color: C.accent }}>Arbitrum Stylus</span>.
          <br />
          Paid in <span style={{ color: C.accent }}>USDG</span>.
        </div>
      </Rise>

      <div style={{ marginTop: 26 }}>
        <Caption at={56}>
          A depth engine in Rust, a sentinel that can only ever lower a cap, and a dashboard that proves
          the numbers live.
        </Caption>
      </div>

      <div style={{ display: 'flex', gap: 110, marginTop: 72 }}>
        <Link at={130} label="Source" url={LINKS.repo} />
        <Link at={160} label="Dashboard" url={LINKS.dashboard} />
      </div>

      <div
        style={{
          marginTop: 34,
          fontFamily: MONO,
          fontSize: 25,
          color: C.dim,
          ...tnum,
          opacity: interpolate(frame, [196, 216], [0, 1], { extrapolateLeft: 'clamp', extrapolateRight: 'clamp' }),
        }}
      >
        engine {ENGINE} on chain {CHAIN_ID}
      </div>

      <div style={{ marginTop: 78 }}>
        <Wordmark at={300} size={128} />
      </div>
    </Scene>
  )
}
