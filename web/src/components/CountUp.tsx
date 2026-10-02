import { useCountUp, useInView } from '../hooks'

/// Counts a figure up from zero once, the first time it is seen. The element reserves its width
/// with tabular numerals, so the digits changing never moves anything around it.
export function CountUp({
  value,
  format,
  ms = 800,
}: {
  value: number
  format: (n: number) => string
  ms?: number
}) {
  const { ref, seen } = useInView<HTMLSpanElement>()
  const shown = useCountUp(value, seen, ms)
  // The final string is what assistive tech reads, not the intermediate frames. Real text in a
  // visually hidden span, rather than aria-label, which ARIA prohibits on a generic element.
  return (
    <span ref={ref} className="countup">
      <span className="vh">{format(value)}</span>
      <span aria-hidden="true">{format(shown)}</span>
    </span>
  )
}
