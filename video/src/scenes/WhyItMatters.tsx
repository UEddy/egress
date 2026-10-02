import { interpolate, useCurrentFrame } from 'remotion'
import { C, tnum } from '../theme'
import { Caption, Eyebrow, Headline, Rise, Scene } from '../ui'
import { TIGHTEST_PCT } from '../data'

const Step: React.FC<{ at: number; n: string; title: string; body: string; accent?: boolean }> = ({
  at,
  n,
  title,
  body,
  accent,
}) => {
  const frame = useCurrentFrame()
  return (
    <div
      style={{
        flex: 1,
        backgroundColor: C.panel,
        border: `1px solid ${accent ? C.accentDim : C.line}`,
        borderRadius: 18,
        padding: 36,
        opacity: interpolate(frame, [at, at + 18], [0, 1], { extrapolateLeft: 'clamp', extrapolateRight: 'clamp' }),
        translate: `0px ${interpolate(frame, [at, at + 18], [16, 0], {
          extrapolateLeft: 'clamp',
          extrapolateRight: 'clamp',
          easing: (t) => 1 - Math.pow(1 - t, 3),
        })}px`,
      }}
    >
      <div style={{ fontSize: 24, color: accent ? C.accent : C.dim, fontWeight: 700, marginBottom: 16, ...tnum }}>{n}</div>
      <div style={{ fontSize: 36, fontWeight: 620, marginBottom: 14, color: accent ? C.accent : C.text }}>{title}</div>
      <div style={{ fontSize: 27, color: C.dim, lineHeight: 1.5 }}>{body}</div>
    </div>
  )
}

/** 0:35 to 0:55. What the gap costs, in three plain steps. */
export const WhyItMatters: React.FC = () => (
  <Scene>
    <Eyebrow at={0}>Why it matters</Eyebrow>
    <Headline at={6} size={72}>
      A loan is only as good as the price you can actually sell at.
    </Headline>

    <div style={{ display: 'flex', gap: 26, marginTop: 70 }}>
      <Step at={80} n="1" title="The price falls" body="A stock drops. Positions go underwater and have to be liquidated." />
      <Step at={116} n="2" title="Everyone sells into one pool" body={`Selling that much moves the price far past ${TIGHTEST_PCT}. The quoted price was never available.`} />
      <Step at={152} n="3" title="Lenders eat the difference" body="The collateral sells for less than the debt. What is left over is bad debt." accent />
    </div>

    <Rise at={330} dur={20}>
      <div style={{ marginTop: 60, fontSize: 44, fontWeight: 620, lineHeight: 1.3, maxWidth: 1500 }}>
        The time to stop that is <span style={{ color: C.accent }}>before the crash</span>, not during it.
      </div>
    </Rise>

    <div style={{ marginTop: 20 }}>
      <Caption at={390}>So the limit has to come from depth, measured continuously, not from a guess.</Caption>
    </div>
  </Scene>
)
