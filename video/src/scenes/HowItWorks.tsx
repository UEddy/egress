import { interpolate, spring, useCurrentFrame, useVideoConfig } from 'remotion'
import { C, MONO, tnum } from '../theme'
import { Caption, Eyebrow, Headline, Rise, Scene } from '../ui'
import { DEPTH_BLOCK, TIGHTEST_PCT, snap } from '../data'

const POOLS = snap.stocks.reduce((n, s) => n + s.pools.length, 0)

const Card: React.FC<{
  at: number
  step: string
  title: string
  body: string
  children?: React.ReactNode
}> = ({ at, step, title, body, children }) => {
  const frame = useCurrentFrame()
  const { fps } = useVideoConfig()
  const s = spring({ frame: frame - at, fps, durationInFrames: 24, config: { damping: 200 } })
  return (
    <div
      style={{
        flex: 1,
        backgroundColor: C.panel,
        border: `1px solid ${C.line}`,
        borderRadius: 20,
        padding: 34,
        minHeight: 420,
        opacity: interpolate(frame, [at, at + 16], [0, 1], { extrapolateLeft: 'clamp', extrapolateRight: 'clamp' }),
        scale: String(0.97 + s * 0.03),
      }}
    >
      <div style={{ fontSize: 22, color: C.accent, fontWeight: 700, letterSpacing: '0.12em', marginBottom: 18 }}>
        {step}
      </div>
      <div style={{ fontSize: 37, fontWeight: 620, marginBottom: 14, lineHeight: 1.18 }}>{title}</div>
      <div style={{ fontSize: 26, color: C.dim, lineHeight: 1.5, marginBottom: 22 }}>{body}</div>
      {children}
    </div>
  )
}

/** A row of ticks being walked, standing in for the engine crossing a pool's liquidity. */
const TickWalk: React.FC<{ at: number }> = ({ at }) => {
  const frame = useCurrentFrame()
  const n = 22
  return (
    <div style={{ display: 'flex', gap: 5, alignItems: 'flex-end', height: 86 }}>
      {new Array(n).fill(0).map((_, i) => {
        const lit = interpolate(frame, [at + i * 3.4, at + i * 3.4 + 14], [0, 1], {
          extrapolateLeft: 'clamp',
          extrapolateRight: 'clamp',
        })
        return (
          <div
            key={i}
            style={{
              flex: 1,
              height: 26 + ((i * 37) % 58),
              borderRadius: 3,
              backgroundColor: C.line2,
              opacity: 0.35 + lit * 0.65,
              scale: `1 ${0.6 + lit * 0.4}`,
              transformOrigin: 'bottom center',
              ...(lit > 0.5 ? { backgroundColor: C.accent } : {}),
            }}
          />
        )
      })}
    </div>
  )
}

/** Five readings landing, with the median one picked out. */
const Median: React.FC<{ at: number }> = ({ at }) => {
  const frame = useCurrentFrame()
  const vals = [0.62, 0.95, 0.78, 0.44, 0.86]
  const medianIdx = 2
  return (
    <div style={{ display: 'flex', gap: 12, alignItems: 'flex-end', height: 86 }}>
      {vals.map((v, i) => {
        const up = interpolate(frame, [at + i * 9, at + i * 9 + 16], [0, 1], {
          extrapolateLeft: 'clamp',
          extrapolateRight: 'clamp',
          easing: (t) => 1 - Math.pow(1 - t, 3),
        })
        const chosen = i === medianIdx && frame > at + 70
        return (
          <div
            key={i}
            style={{
              flex: 1,
              height: 86 * v,
              borderRadius: 6,
              backgroundColor: chosen ? C.accent : C.line2,
              scale: `1 ${up}`,
              transformOrigin: 'bottom center',
              opacity: frame > at + 70 && !chosen ? 0.4 : 1,
            }}
          />
        )
      })}
    </div>
  )
}

/** A cap bar being lowered, never raised. */
const CapCut: React.FC<{ at: number }> = ({ at }) => {
  const frame = useCurrentFrame()
  const { fps } = useVideoConfig()
  const cut = spring({ frame: frame - at, fps, durationInFrames: 30, config: { damping: 200 } })
  return (
    <div style={{ height: 86, display: 'flex', alignItems: 'center' }}>
      <div
        style={{
          width: '100%',
          height: 46,
          borderRadius: 9,
          backgroundColor: C.panel2,
          border: `1px solid ${C.line}`,
          overflow: 'hidden',
        }}
      >
        <div
          style={{
            height: '100%',
            width: `${100 - cut * 62}%`,
            backgroundColor: C.accent,
            borderRadius: 8,
          }}
        />
      </div>
    </div>
  )
}

/** 0:55 to 1:25. Three steps: walk the pools, take the median, lower the cap. */
export const HowItWorks: React.FC = () => {
  const frame = useCurrentFrame()
  return (
    <Scene>
      <Eyebrow at={0}>How Egress works</Eyebrow>
      <Headline at={6} size={70}>
        Measure what can really be sold, then cap lending to it.
      </Headline>

      <div style={{ display: 'flex', gap: 24, marginTop: 56 }}>
        <Card
          at={70}
          step="STEP 1"
          title="Walk the real pools"
          body={`An Arbitrum Stylus contract crosses every initialized tick in ${POOLS} Uniswap V3 pools, exactly as a swap would, and returns what a seller receives before the price falls ${TIGHTEST_PCT}.`}
        >
          <TickWalk at={120} />
        </Card>
        <Card
          at={250}
          step="STEP 2"
          title="Take the median of five"
          body="Keepers record readings spaced apart in time. The guard cuts to the median of five, so one or two manipulated readings can neither trigger a cut nor block one."
        >
          <Median at={300} />
        </Card>
        <Card
          at={430}
          step="STEP 3"
          title="Lower the vault's cap"
          body="Installed as a Morpho Vault V2 sentinel, the guard lowers the supply cap for that stock. New lending stops. Open positions are untouched."
        >
          <CapCut at={500} />
        </Card>
      </div>

      <Rise at={640} dur={22}>
        <div
          style={{
            marginTop: 54,
            display: 'flex',
            gap: 18,
            flexWrap: 'wrap',
          }}
        >
          {['Can never raise a cap.', 'Never moves funds.'].map((t, i) => (
            <div
              key={t}
              style={{
                fontSize: 44,
                fontWeight: 640,
                color: C.accent,
                backgroundColor: C.accentDim,
                borderRadius: 999,
                padding: '14px 36px',
                opacity: interpolate(frame, [640 + i * 16, 640 + i * 16 + 18], [0, 1], {
                  extrapolateLeft: 'clamp',
                  extrapolateRight: 'clamp',
                }),
              }}
            >
              {t}
            </div>
          ))}
        </div>
      </Rise>

      <div style={{ marginTop: 22, fontFamily: MONO, fontSize: 23, color: C.dim, ...tnum }}>
        <Caption at={720}>
          <span style={{ fontFamily: MONO, fontSize: 23 }}>depth measured at block {DEPTH_BLOCK.toLocaleString('en-US')}</span>
        </Caption>
      </div>
    </Scene>
  )
}
