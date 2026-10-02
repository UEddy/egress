import { AbsoluteFill, interpolate, useCurrentFrame } from 'remotion'
import { C, FONT, tnum } from '../theme'
import { Bar, Caption, Counter, Eyebrow, Headline, Rise } from '../ui'
import { AAPL, TIGHTEST_PCT, TOTAL, fmt } from '../data'

// Two beats rather than one crowded frame: the chain wide total, then AAPL on its own.
// They cross fade, so neither has to compete for vertical space.
const BEAT_OUT = 250
const BEAT_IN = 262

/** 0:10 to 0:35. Thin liquidity, the chain wide total, then AAPL as two bars. */
export const Problem: React.FC = () => {
  const frame = useCurrentFrame()

  // Both bars share one scale, so their lengths are comparable by eye.
  const max = Math.max(AAPL.lentF, AAPL.sellableF)

  return (
    <AbsoluteFill
      style={{ backgroundColor: C.bg, color: C.text, fontFamily: FONT, padding: 130, justifyContent: 'center' }}
    >
      {/* Beat one: the scale of it. */}
      <AbsoluteFill
        style={{
          padding: 130,
          justifyContent: 'center',
          opacity: interpolate(frame, [BEAT_OUT, BEAT_OUT + 22], [1, 0], {
            extrapolateLeft: 'clamp',
            extrapolateRight: 'clamp',
          }),
        }}
      >
        <Eyebrow at={0}>The problem</Eyebrow>
        <Headline at={6} size={74}>
          Stock tokens trade around the clock. The liquidity behind them does not.
        </Headline>

        <div
          style={{
            marginTop: 70,
            opacity: interpolate(frame, [64, 82], [0, 1], { extrapolateLeft: 'clamp', extrapolateRight: 'clamp' }),
          }}
        >
          <div style={{ fontSize: 27, letterSpacing: '0.14em', textTransform: 'uppercase', color: C.dim, fontWeight: 600 }}>
            Stock collateral in lending on Robinhood Chain
          </div>
          <div style={{ display: 'flex', alignItems: 'baseline', gap: 24, marginTop: 16 }}>
            <Counter
              to={TOTAL.lentF}
              at={70}
              dur={54}
              style={{ fontSize: 168, fontWeight: 660, letterSpacing: '-0.035em', lineHeight: 1 }}
            />
            <span style={{ fontSize: 48, color: C.dim, fontWeight: 500 }}>USDG</span>
          </div>
          <div style={{ fontSize: 29, color: C.dim, marginTop: 16, ...tnum }}>
            across {TOTAL.holders} contracts in {TOTAL.protocols} lending protocols, at block{' '}
            {TOTAL.block.toLocaleString('en-US')}
          </div>
        </div>
      </AbsoluteFill>

      {/* Beat two: one stock, measured against its own depth. */}
      <AbsoluteFill
        style={{
          padding: 130,
          justifyContent: 'center',
          opacity: interpolate(frame, [BEAT_IN, BEAT_IN + 22], [0, 1], {
            extrapolateLeft: 'clamp',
            extrapolateRight: 'clamp',
          }),
        }}
      >
        <Eyebrow at={BEAT_IN}>Take one stock. AAPL.</Eyebrow>
        <Headline at={BEAT_IN + 6} size={62}>
          More is pledged to lenders than the market can absorb.
        </Headline>

        <div style={{ marginTop: 64 }}>
          <Bar
            at={BEAT_IN + 40}
            widthPct={(AAPL.lentF / max) * 100}
            color={C.text}
            label="Pledged to lenders"
            value={`${fmt(AAPL.lentF)} USDG`}
          />
          <Bar
            at={BEAT_IN + 86}
            widthPct={(AAPL.sellableF / max) * 100}
            color={C.accent}
            label={`Actually sellable within ${TIGHTEST_PCT}`}
            value={`${fmt(AAPL.sellableF)} USDG`}
          />
        </div>

        <Rise at={BEAT_IN + 150} dur={20}>
          <div style={{ fontSize: 52, color: C.accent, fontWeight: 640, ...tnum }}>
            A gap of {fmt(AAPL.gapF)} USDG.
          </div>
        </Rise>

        <div style={{ marginTop: 22 }}>
          <Caption at={BEAT_IN + 186}>
            Both figures measured in the same run, so the comparison sits at one block.
          </Caption>
        </div>
      </AbsoluteFill>
    </AbsoluteFill>
  )
}
