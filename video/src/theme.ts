// The dashboard's visual language, so the video and the site read as one thing.
// Mirrors web/src/styles.css.
export const C = {
  bg: '#0b0e13',
  panel: '#12161d',
  panel2: '#171c25',
  line: '#222833',
  line2: '#2c3542',
  text: '#e8ebf0',
  dim: '#99a3b4',
  accent: '#ffab5e',
  accentDim: '#3a2a1c',
} as const

export const FONT = 'Inter var'
export const MONO = 'ui-monospace, SFMono-Regular, Menlo, Consolas, monospace'

// The dashboard's ease-out curve. Every move in the video uses it.
export const EASE = [0.22, 1, 0.36, 1] as const

// Nothing animates faster than this. 12 frames at 30fps is 0.4s.
export const MIN_MOVE = 12

export const tnum = { fontVariantNumeric: 'tabular-nums' } as const
