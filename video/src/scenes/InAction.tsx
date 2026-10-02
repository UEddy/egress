import { Video } from '@remotion/media'
import { interpolate, spring, staticFile, useCurrentFrame, useVideoConfig } from 'remotion'
import { C, tnum } from '../theme'
import { Counter, Eyebrow, Headline, Rise, Scene } from '../ui'
import { FORK, fmt } from '../data'

/** 1:45 to 2:10. A real fork run, then the cap figures it produced. */
export const InAction: React.FC = () => {
  const frame = useCurrentFrame()
  const { fps } = useVideoConfig()
  const drop = spring({ frame: frame - 470, fps, durationInFrames: 36, config: { damping: 200 } })

  return (
    <Scene pad={0}>
      <div style={{ padding: '0 130px', opacity: interpolate(frame, [0, 16, 96, 112], [0, 1, 1, 0], { extrapolateLeft: 'clamp', extrapolateRight: 'clamp' }) }}>
        <Eyebrow at={0}>The guard in action</Eyebrow>
        <Headline at={8} size={66}>
          Run against the real vault, on a fork of mainnet.
        </Headline>
      </div>

      {/* The actual run. Nothing staged: this is the script's own output. */}
      <div
        style={{
          position: 'absolute',
          inset: 0,
          opacity: interpolate(frame, [104, 126, 404, 428], [0, 1, 1, 0], { extrapolateLeft: 'clamp', extrapolateRight: 'clamp' }),
        }}
      >
        <Video src={staticFile('fork-sim.mp4')} muted style={{ width: '100%', height: '100%', objectFit: 'cover' }} />
      </div>

      {/* The figures that run produced. */}
      <div
        style={{
          position: 'absolute',
          inset: 0,
          padding: '0 130px',
          display: 'flex',
          flexDirection: 'column',
          justifyContent: 'center',
          opacity: interpolate(frame, [418, 444], [0, 1], { extrapolateLeft: 'clamp', extrapolateRight: 'clamp' }),
        }}
      >
        <div style={{ fontSize: 30, letterSpacing: '0.14em', textTransform: 'uppercase', color: C.dim, fontWeight: 600 }}>
          NetNet Credit Vault V2, AAPL cap, at block {FORK.block.toLocaleString('en-US')}
        </div>

        <div style={{ display: 'flex', alignItems: 'center', gap: 64, marginTop: 38 }}>
          <div>
            <div style={{ fontSize: 28, color: C.dim, marginBottom: 10 }}>Cap before</div>
            <div style={{ fontSize: 110, fontWeight: 650, letterSpacing: '-0.03em', ...tnum }}>
              {fmt(FORK.capBefore)}
            </div>
          </div>
          <div
            style={{
              fontSize: 90,
              color: C.dim,
              opacity: interpolate(frame, [462, 480], [0, 1], { extrapolateLeft: 'clamp', extrapolateRight: 'clamp' }),
              translate: `${interpolate(frame, [462, 486], [-22, 0], { extrapolateLeft: 'clamp', extrapolateRight: 'clamp' })}px 0px`,
            }}
          >
            &rarr;
          </div>
          <div>
            <div style={{ fontSize: 28, color: C.dim, marginBottom: 10 }}>Cap after five readings</div>
            <div style={{ fontSize: 110, fontWeight: 650, letterSpacing: '-0.03em', color: C.accent, ...tnum }}>
              <Counter to={FORK.capAfter} at={470} dur={38} />
            </div>
          </div>
        </div>

        <div
          style={{
            marginTop: 40,
            height: 10,
            borderRadius: 999,
            backgroundColor: C.panel2,
            overflow: 'hidden',
            maxWidth: 1400,
          }}
        >
          <div
            style={{
              height: '100%',
              width: `${100 - (1 - FORK.capAfter / FORK.capBefore) * 100 * drop}%`,
              backgroundColor: C.accent,
              borderRadius: 999,
            }}
          />
        </div>

        <Rise at={520} dur={22}>
          <div style={{ marginTop: 44, fontSize: 38, color: C.dim, lineHeight: 1.45, maxWidth: 1500 }}>
            Against {fmt(FORK.depth)} USDG of depth within 5%, with {fmt(FORK.allocation)} USDG already lent.
            The new cap sits below what is already outstanding, so the cut stops new lending and leaves
            open positions alone. The guard could not raise it back. The curator restored it only after
            the {FORK.timelockDays} day timelock.
          </div>
        </Rise>
      </div>
    </Scene>
  )
}
