import type { ReactNode } from 'react'
import { useInView } from '../hooks'

/// Fades and rises its children once, the first time they reach the viewport.
/// The motion is pure CSS on opacity and transform, so it never triggers layout.
/// `index` staggers siblings; the stylesheet turns it into a delay.
export function Reveal({
  children,
  index = 0,
  as: Tag = 'div',
  className = '',
  id,
  ariaLabelledby,
  role,
}: {
  children: ReactNode
  index?: number
  as?: 'div' | 'section' | 'li'
  className?: string
  id?: string
  ariaLabelledby?: string
  role?: string
}) {
  const { ref, seen } = useInView<HTMLElement>()
  return (
    <Tag
      ref={ref as never}
      id={id}
      role={role}
      aria-labelledby={ariaLabelledby}
      className={`reveal ${seen ? 'in' : ''} ${className}`.trim()}
      style={{ ['--i' as string]: index }}
    >
      {children}
    </Tag>
  )
}
