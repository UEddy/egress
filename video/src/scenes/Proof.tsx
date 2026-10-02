import { Video } from '@remotion/media'
import { interpolate, staticFile, useCurrentFrame } from 'remotion'
import { C, MONO, tnum } from '../theme'
import { Caption, Eyebrow, Headline, Rise, Scene } from '../ui'
import { CHAIN_ID, DEPTH_BLOCK, ENGINE, TIGHTEST_PCT, VERIFY } from '../data'

/** 1:25 to 1:45. The engine is live, the readings match real swaps, and here it is running. */
export const Proof: React.FC = () => {
  const frame = useCurrentFrame()
  return (
    <Scene pad={0}>
      <div style={{ padding: '0 130px' }}>
        <Eyebrow at={0}>Proof</Eyebrow>
        <Headline at={6} size={66}>
          The engine is deployed and its answers match reality.
        </Headline>

        <Rise at={44} dur={20}>
          <div
            style={{
              marginTop: 34,
              display: 'inline-block',
              backgroundColor: C.panel,
              border: `1px solid ${C.line}`,
              borderRadius: 14,
              padding: '20px 28px',
            }}
          >
            <div style={{ fontSize: 21, letterSpacing: '0.13em', textTransform: 'uppercase', color: C.dim, fontWeight: 600 }}>
              Depth engine, Arbitrum Stylus, chain {CHAIN_ID}
            </div>
            <div style={{ fontFamily: MONO, fontSize: 32, marginTop: 10, ...tnum }}>{ENGINE}</div>
          </div>
        </Rise>

        <Rise at={86} dur={20}>
          <div style={{ marginTop: 28, fontSize: 52, fontWeight: 640 }}>
            <span style={{ color: C.accent, ...tnum }}>
              {VERIFY.matched} of {VERIFY.cases}
            </span>{' '}
            live readings match real swaps to the wei.
            {VERIFY.mismatched === 0 ? ' Nothing mismatched.' : ` ${VERIFY.mismatched} mismatched.`}
          </div>
        </Rise>

        <div style={{ marginTop: 14 }}>
          <Caption at={120}>
            Every pool at every bound, re-asked of the deployed engine at block{' '}
            {VERIFY.block.toLocaleString('en-US')} and recorded in {VERIFY.file}.
          </Caption>
        </div>
      </div>

      {/* The real dashboard, calling the deployed engine from a browser. */}
      <div
        style={{
          position: 'absolute',
          inset: 0,
          opacity: interpolate(frame, [240, 262], [0, 1], { extrapolateLeft: 'clamp', extrapolateRight: 'clamp' }),
        }}
      >
        <Video
          src={staticFile('live-check.mp4')}
          loop
          muted
          style={{ width: '100%', height: '100%', objectFit: 'cover' }}
        />
        <div
          style={{
            position: 'absolute',
            left: 0,
            right: 0,
            bottom: 0,
            padding: '110px 130px 54px',
            background: `linear-gradient(to top, ${C.bg} 22%, transparent)`,
          }}
        >
          <div style={{ fontSize: 38, fontWeight: 600 }}>
            Check it yourself: the dashboard calls the deployed engine live, at {TIGHTEST_PCT}, from
            your browser.
          </div>
          <div style={{ fontSize: 29, color: C.dim, marginTop: 12, fontFamily: MONO, ...tnum }}>
            snapshot block {DEPTH_BLOCK.toLocaleString('en-US')} vs live: the difference is depth
            moving between blocks, not an error.
          </div>
        </div>
      </div>
    </Scene>
  )
}
