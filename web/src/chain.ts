import { createPublicClient, defineChain, http, type Address } from 'viem'

// The browser only ever talks to Robinhood Chain's public endpoint. No private or keyed RPC URL
// belongs in a static site: anything shipped here is readable by every visitor. Nothing in this
// file may be read from an environment variable for that reason.
export const PUBLIC_RPC = 'https://rpc.mainnet.chain.robinhood.com'

export const EXPLORER = 'https://robinhoodchain.blockscout.com'
export const REPO = 'https://github.com/UEddy/exitline'

export const robinhoodChain = defineChain({
  id: 4663,
  name: 'Robinhood Chain',
  nativeCurrency: { name: 'Ether', symbol: 'ETH', decimals: 18 },
  rpcUrls: { default: { http: [PUBLIC_RPC] } },
  blockExplorers: { default: { name: 'Blockscout', url: EXPLORER } },
})

export const client = createPublicClient({ chain: robinhoodChain, transport: http(PUBLIC_RPC) })

export const engineAbi = [
  {
    type: 'function',
    name: 'sellProceeds',
    stateMutability: 'view',
    inputs: [
      { name: 'pool', type: 'address' },
      { name: 'stockIsToken0', type: 'bool' },
      { name: 'maxImpactBps', type: 'uint256' },
    ],
    outputs: [{ name: '', type: 'uint256' }],
  },
] as const

export type LivePool = { pool: Address; stockIsToken0: boolean }

/// Calls the deployed engine once per pool, one at a time. Sequential on purpose: the public
/// endpoint throttles bursts, and a handful of reads in series is well inside what it allows.
export async function sellProceedsTotal(
  engine: Address,
  pools: LivePool[],
  impactBps: number,
): Promise<{ total: bigint; perPool: bigint[] }> {
  const perPool: bigint[] = []
  for (const p of pools) {
    const out = await client.readContract({
      address: engine,
      abi: engineAbi,
      functionName: 'sellProceeds',
      args: [p.pool, p.stockIsToken0, BigInt(impactBps)],
    })
    perPool.push(out as bigint)
  }
  return { total: perPool.reduce((a, b) => a + b, 0n), perPool }
}

/// Turns an RPC failure into something a reader can act on, with throttling called out by name.
export function describeError(e: unknown): string {
  const raw = e instanceof Error ? e.message : String(e)
  const text = raw.toLowerCase()
  if (text.includes('429') || text.includes('rate limit') || text.includes('too many requests')) {
    return 'The public RPC is rate limiting right now. Wait a few seconds and check again.'
  }
  if (text.includes('timeout') || text.includes('timed out')) {
    return 'The public RPC did not answer in time. Try again in a moment.'
  }
  if (text.includes('fetch') || text.includes('network') || text.includes('failed to fetch')) {
    return 'Could not reach the public RPC from this browser. Check your connection and try again.'
  }
  return 'The call failed: ' + raw.split('\n')[0]
}
