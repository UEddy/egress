import { useEffect, useRef, useState } from 'react'

/// Reads the user's motion preference and keeps up with changes to it.
export function useReducedMotion(): boolean {
  const [reduced, setReduced] = useState(
    () => typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches,
  )
  useEffect(() => {
    const mq = matchMedia('(prefers-reduced-motion: reduce)')
    const on = () => setReduced(mq.matches)
    mq.addEventListener('change', on)
    return () => mq.removeEventListener('change', on)
  }, [])
  return reduced
}

/// Marks an element as seen the first time it enters the viewport, once and never again.
/// The animation itself is CSS, so nothing here touches layout.
export function useInView<T extends Element>(rootMargin = '0px 0px -8% 0px') {
  const ref = useRef<T | null>(null)
  const [seen, setSeen] = useState(false)
  useEffect(() => {
    const el = ref.current
    if (!el || seen) return
    if (typeof IntersectionObserver !== 'function') {
      setSeen(true)
      return
    }
    const io = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) {
          setSeen(true)
          io.disconnect()
        }
      },
      { rootMargin, threshold: 0.01 },
    )
    io.observe(el)
    return () => io.disconnect()
  }, [seen, rootMargin])
  return { ref, seen }
}

/// Counts from zero to `to` once, over `ms`, on an ease-out curve. Returns `to` immediately when
/// the user asked for reduced motion or the value has not been revealed yet.
export function useCountUp(to: number, run: boolean, ms = 800): number {
  const reduced = useReducedMotion()
  const [value, setValue] = useState(() => (reduced ? to : 0))
  const done = useRef(false)
  useEffect(() => {
    if (!run || done.current) return
    if (reduced || ms <= 0) {
      done.current = true
      setValue(to)
      return
    }
    let raf = 0
    const start = performance.now()
    const tick = (now: number) => {
      const t = Math.min(1, (now - start) / ms)
      // Same ease-out the CSS uses, so motion across the page feels like one system.
      const eased = 1 - Math.pow(1 - t, 3)
      setValue(to * eased)
      if (t < 1) raf = requestAnimationFrame(tick)
      else done.current = true
    }
    raf = requestAnimationFrame(tick)
    return () => cancelAnimationFrame(raf)
  }, [run, to, ms, reduced])
  return value
}
