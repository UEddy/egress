import { encodeFunctionData, decodeFunctionResult } from 'viem'

// The browser only ever talks to Robinhood Chain's public endpoint. No private or keyed RPC URL
// belongs in a static site: everything shipped here is readable by every visitor. Nothing in this
// file may come from an environment variable, for that reason.
export const PUBLIC_RPC = 'https://rpc.mainnet.chain.robinhood.com'
export const EXPLORER = 'https://robinhoodchain.blockscout.com'
export const REPO = 'https://github.com/UEddy/exitline'

// Only viem's ABI codec is imported, not createPublicClient: the one call this page makes is a
// single eth_call, and the client machinery would cost more than the whole rest of the bundle.
const engineAbi = [
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

export type LivePool = { pool: string; stockIsToken0: boolean }

class RpcError extends Error {
  constructor(
    message: string,
    readonly throttled: boolean,
  ) {
    super(message)
  }
}

async function ethCall(to: string, data: string, signal: AbortSignal): Promise<string> {
  let res: Response
  try {
    res = await fetch(PUBLIC_RPC, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'eth_call', params: [{ to, data }, 'latest'] }),
      signal,
    })
  } catch (e) {
    if ((e as Error)?.name === 'AbortError') throw e
    throw new RpcError('network', false)
  }
  if (res.status === 429) throw new RpcError('throttled', true)
  if (!res.ok) throw new RpcError(`http ${res.status}`, res.status === 503)
  const body = (await res.json()) as { result?: string; error?: { code?: number; message?: string } }
  if (body.error) {
    const msg = body.error.message ?? 'call failed'
    throw new RpcError(msg, body.error.code === 429 || /rate|limit|too many/i.test(msg))
  }
  if (!body.result) throw new RpcError('empty response', false)
  return body.result
}

/// Calls the deployed engine once per pool, in sequence. Sequential on purpose: the public endpoint
/// throttles bursts, and a few reads in series sit well inside what it allows.
export async function sellProceedsTotal(
  engine: string,
  pools: LivePool[],
  impactBps: number,
  signal: AbortSignal,
): Promise<bigint> {
  let total = 0n
  for (const p of pools) {
    const data = encodeFunctionData({
      abi: engineAbi,
      functionName: 'sellProceeds',
      args: [p.pool as `0x${string}`, p.stockIsToken0, BigInt(impactBps)],
    })
    const raw = await ethCall(engine, data, signal)
    total += decodeFunctionResult({
      abi: engineAbi,
      functionName: 'sellProceeds',
      data: raw as `0x${string}`,
    }) as bigint
  }
  return total
}

/// Plain words for a failure, with throttling named rather than dressed up as something else.
export function describeError(e: unknown): string {
  // Short on purpose: the result slot reserves its height in advance, and a long sentence would
  // be the one thing that could still push the layout around.
  if (e instanceof RpcError) {
    if (e.throttled) return 'Rate limited, retry shortly'
    if (e.message === 'network') return 'Could not reach the RPC'
    return 'Call failed, retry'
  }
  if ((e as Error)?.name === 'AbortError') return 'Cancelled'
  return 'Call failed, retry'
}
