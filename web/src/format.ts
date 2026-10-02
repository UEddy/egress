// Integer strings in token units to something readable. Display only: the comparisons that decide
// whether a stock is flagged are done in BigInt, never on these floats.

export function toFloat(value: string | bigint, decimals: number): number {
  const v = typeof value === 'bigint' ? value : BigInt(value || '0')
  const scale = 10n ** BigInt(decimals)
  const whole = v / scale
  const frac = v % scale
  return Number(whole) + Number(frac) / Number(scale)
}

export function usdg(value: string | bigint | null, decimals = 6, maxFrac = 0): string {
  if (value === null || value === undefined) return 'n/a'
  return toFloat(value, decimals).toLocaleString('en-US', {
    minimumFractionDigits: 0,
    maximumFractionDigits: maxFrac,
  })
}

/// Short form for the headline figures: 1.2M, 660k, 940.
export function compact(value: string | bigint | null, decimals = 6): string {
  if (value === null || value === undefined) return 'n/a'
  const n = toFloat(value, decimals)
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(2) + 'M'
  if (n >= 10_000) return Math.round(n / 1000).toLocaleString('en-US') + 'k'
  if (n >= 1000) return (n / 1000).toFixed(1) + 'k'
  return n.toLocaleString('en-US', { maximumFractionDigits: 2 })
}

export function units(value: string | bigint | null, decimals: number, maxFrac = 4): string {
  if (value === null || value === undefined) return 'n/a'
  return toFloat(value, decimals).toLocaleString('en-US', { maximumFractionDigits: maxFrac })
}

/// lent / sellable, as a multiple. Null when there is no depth to divide by.
export function ratio(lent: string | null, sellable: string): number | null {
  const d = BigInt(sellable || '0')
  if (d === 0n) return null
  const n = BigInt(lent || '0')
  // Four decimal places of the quotient, computed in integers first.
  return Number((n * 10_000n) / d) / 10_000
}

export function pct(bps: number): string {
  return bps / 100 + '%'
}

export function shortAddr(a: string): string {
  return a.slice(0, 6) + '...' + a.slice(-4)
}

export function lltv(wad: string): string {
  return (toFloat(wad, 18) * 100).toFixed(1) + '%'
}
