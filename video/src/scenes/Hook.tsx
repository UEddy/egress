import { interpolate, useCurrentFrame } from 'remotion'
import { C } from '../theme'
import { Caption, Rise, Scene, Wordmark } from '../ui'
import { CHAIN_ID } from '../data'

/** 0:00 to 0:10. The claim, then the name. */
export const Hook: React.FC = () => {
  const frame = useCurrentFrame()
  return (
    <Scene>
      <Rise at={6} dur={22} y={24}>
        <div style={{ fontSize: 96, lineHeight: 1.08, letterSpacing: '-0.03em', fontWeight: 650, maxWidth: 1600 }}>
          Lenders on Robinhood Chain are lending against stock that
          <span style={{ color: C.accent }}> cannot be sold.</span>
        </div>
      </Rise>

      <div style={{ marginTop: 44 }}>
        <Caption at={54}>Measured onchain, block by block, on chain id {CHAIN_ID}.</Caption>
      </div>

      <div
        style={{
          marginTop: 86,
          opacity: interpolate(frame, [150, 170], [0, 1], {
            extrapolateLeft: 'clamp',
            extrapolateRight: 'clamp',
          }),
        }}
      >
        <Wordmark at={150} />
      </div>
    </Scene>
  )
}
