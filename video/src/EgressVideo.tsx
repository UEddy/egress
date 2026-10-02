import React from 'react'
import { AbsoluteFill } from 'remotion'
import { TransitionSeries, linearTiming } from '@remotion/transitions'
import { fade } from '@remotion/transitions/fade'
import { C } from './theme'
import { Hook } from './scenes/Hook'
import { Problem } from './scenes/Problem'
import { WhyItMatters } from './scenes/WhyItMatters'
import { HowItWorks } from './scenes/HowItWorks'
import { Proof } from './scenes/Proof'
import { InAction } from './scenes/InAction'
import { Close } from './scenes/Close'

// 30fps throughout.
//
// Scenes cross dissolve rather than cut. Without the overlap each scene opened on an empty frame
// while its first element faded up, which read as a flicker at every boundary.
//
// A transition consumes its duration from the timeline, so the scene durations below sum to
// 4590 and the six 15 frame dissolves bring the total back to exactly 4500 frames, which is 2:30.
export const TRANSITION = 15

export const SCENES = [
  { name: 'Hook', durationInFrames: 300, component: Hook },
  { name: 'Problem', durationInFrames: 765, component: Problem },
  { name: 'WhyItMatters', durationInFrames: 615, component: WhyItMatters },
  { name: 'HowItWorks', durationInFrames: 915, component: HowItWorks },
  { name: 'Proof', durationInFrames: 615, component: Proof },
  { name: 'InAction', durationInFrames: 765, component: InAction },
  { name: 'Close', durationInFrames: 615, component: Close },
] as const

export const TOTAL_FRAMES =
  SCENES.reduce((n, s) => n + s.durationInFrames, 0) - (SCENES.length - 1) * TRANSITION

export const EgressVideo: React.FC = () => (
  <AbsoluteFill style={{ backgroundColor: C.bg }}>
    <TransitionSeries>
      {SCENES.map(({ name, durationInFrames, component: Component }, i) => (
        <React.Fragment key={name}>
          {i > 0 ? (
            <TransitionSeries.Transition
              presentation={fade()}
              timing={linearTiming({ durationInFrames: TRANSITION })}
            />
          ) : null}
          <TransitionSeries.Sequence name={name} durationInFrames={durationInFrames}>
            <Component />
          </TransitionSeries.Sequence>
        </React.Fragment>
      ))}
    </TransitionSeries>
  </AbsoluteFill>
)
