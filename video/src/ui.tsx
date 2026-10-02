import React from 'react'
import { AbsoluteFill, interpolate, spring, useCurrentFrame, useVideoConfig } from 'remotion'
import { C, FONT, MONO, tnum } from './theme'

/** Fade and rise, the dashboard's entrance. Never shorter than 12 frames. */
export const Rise: React.FC<{
  at?: number
  dur?: number
  y?: number
  children: React.ReactNode
  style?: React.CSSProperties
}> = ({ at = 0, dur = 18, y = 18, children, style }) => {
  const frame = useCurrentFrame()
  return (
    <div
      style={{
        ...style,
        opacity: interpolate(frame, [at, at + dur], [0, 1], {
          extrapolateLeft: 'clamp',
          extrapolateRight: 'clamp',
          easing: (t) => t,
        }),
        translate: `0px ${interpolate(frame, [at, at + dur], [y, 0], {
          extrapolateLeft: 'clamp',
          extrapolateRight: 'clamp',
          easing: (t) => 1 - Math.pow(1 - t, 3),
        })}px`,
      }}
    >
      {children}
    </div>
  )
}

export const Scene: React.FC<{ children: React.ReactNode; pad?: number }> = ({
  children,
  pad = 130,
}) => (
  <AbsoluteFill
    style={{
      backgroundColor: C.bg,
      color: C.text,
      fontFamily: FONT,
      padding: pad,
      justifyContent: 'center',
    }}
  >
    {children}
  </AbsoluteFill>
)

/** The caption line that carries the meaning when the sound is off. */
export const Caption: React.FC<{ at?: number; children: React.ReactNode; accent?: boolean }> = ({
  at = 0,
  children,
  accent,
}) => (
  <Rise at={at} dur={16}>
    <div
      style={{
        fontSize: 40,
        lineHeight: 1.42,
        color: accent ? C.accent : C.dim,
        maxWidth: 1400,
        fontWeight: 450,
      }}
    >
      {children}
    </div>
  </Rise>
)

export const Eyebrow: React.FC<{ at?: number; children: React.ReactNode }> = ({
  at = 0,
  children,
}) => (
  <Rise at={at} dur={14}>
    <div
      style={{
        fontSize: 23,
        letterSpacing: '0.16em',
        textTransform: 'uppercase',
        color: C.dim,
        fontWeight: 600,
        marginBottom: 26,
      }}
    >
      {children}
    </div>
  </Rise>
)

export const Headline: React.FC<{ at?: number; children: React.ReactNode; size?: number }> = ({
  at = 0,
  children,
  size = 86,
}) => (
  <Rise at={at} dur={20} y={22}>
    <div
      style={{
        fontSize: size,
        lineHeight: 1.1,
        letterSpacing: '-0.025em',
        fontWeight: 640,
        maxWidth: 1560,
      }}
    >
      {children}
    </div>
  </Rise>
)

/** A figure that counts up once, on a spring, and holds its final value. */
export const Counter: React.FC<{
  to: number
  at?: number
  dur?: number
  format?: (n: number) => string
  style?: React.CSSProperties
}> = ({ to, at = 0, dur = 34, format = (n) => Math.round(n).toLocaleString('en-US'), style }) => {
  const frame = useCurrentFrame()
  const { fps } = useVideoConfig()
  const p = spring({ frame: frame - at, fps, durationInFrames: dur, config: { damping: 200 } })
  return <span style={{ ...tnum, ...style }}>{format(to * p)}</span>
}

export const Mono: React.FC<{ children: React.ReactNode; style?: React.CSSProperties }> = ({
  children,
  style,
}) => <span style={{ fontFamily: MONO, ...tnum, ...style }}>{children}</span>

export const Wordmark: React.FC<{ at?: number; size?: number }> = ({ at = 0, size = 112 }) => {
  const frame = useCurrentFrame()
  const { fps } = useVideoConfig()
  const s = spring({ frame: frame - at, fps, durationInFrames: 26, config: { damping: 200 } })
  return (
    <div
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: 26,
        opacity: interpolate(frame, [at, at + 14], [0, 1], {
          extrapolateLeft: 'clamp',
          extrapolateRight: 'clamp',
        }),
        scale: String(0.94 + s * 0.06),
      }}
    >
      <div
        style={{
          width: size * 0.17,
          height: size * 0.17,
          borderRadius: 999,
          backgroundColor: C.accent,
          boxShadow: `0 0 ${size * 0.4}px ${C.accent}55`,
        }}
      />
      <div style={{ fontSize: size, fontWeight: 660, letterSpacing: '-0.035em' }}>Egress</div>
    </div>
  )
}

/** Horizontal bar that grows from the left. Used for the lent against sellable comparison. */
export const Bar: React.FC<{
  at: number
  widthPct: number
  color: string
  label: string
  value: string
  sub?: string
  dur?: number
}> = ({ at, widthPct, color, label, value, sub, dur = 30 }) => {
  const frame = useCurrentFrame()
  const { fps } = useVideoConfig()
  const grow = spring({ frame: frame - at, fps, durationInFrames: dur, config: { damping: 200 } })
  return (
    <div style={{ marginBottom: 46 }}>
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'baseline',
          marginBottom: 14,
          opacity: interpolate(frame, [at, at + 14], [0, 1], {
            extrapolateLeft: 'clamp',
            extrapolateRight: 'clamp',
          }),
        }}
      >
        <span style={{ fontSize: 34, color: C.dim, fontWeight: 500 }}>{label}</span>
        <span style={{ fontSize: 44, fontWeight: 640, color, ...tnum }}>{value}</span>
      </div>
      <div
        style={{
          height: 54,
          borderRadius: 10,
          backgroundColor: C.panel2,
          border: `1px solid ${C.line}`,
          overflow: 'hidden',
        }}
      >
        <div
          style={{
            height: '100%',
            width: `${widthPct}%`,
            backgroundColor: color,
            borderRadius: 9,
            transformOrigin: 'left center',
            scale: `${grow} 1`,
          }}
        />
      </div>
      {sub ? (
        <div
          style={{
            fontSize: 25,
            color: C.dim,
            marginTop: 12,
            opacity: interpolate(frame, [at + 16, at + 32], [0, 1], {
              extrapolateLeft: 'clamp',
              extrapolateRight: 'clamp',
            }),
          }}
        >
          {sub}
        </div>
      ) : null}
    </div>
  )
}
