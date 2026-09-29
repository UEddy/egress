#!/usr/bin/env bash
# Fork simulation: ExitlineGuard on the real NetNet Credit Vault V2, cutting its AAPL cap.
# Measures AAPL depth with tools/measure at the latest block, then runs the fork test at that same
# block with a mock engine returning those numbers. Nothing is broadcast.
#
# The public RPC keeps only ~10 minutes of state, so both steps run back to back. Set
# ROBINHOOD_RPC_URL to an archive endpoint to replay an older block with FORK_BLOCK=<n>.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
rpc="${ROBINHOOD_RPC_URL:-https://rpc.mainnet.chain.robinhood.com}"
report="$(mktemp -t exitline-aapl-XXXXXX.json)"
trap 'rm -f "$report"' EXIT

block_args=()
if [[ -n "${FORK_BLOCK:-}" ]]; then block_args=(--block "$FORK_BLOCK"); fi

(cd "$root/tools/measure" && cargo run --release --quiet -- \
    --rpc "$rpc" "${block_args[@]}" --only AAPL --impacts 500 --no-lenders --json "$report" >/dev/null)

eval "$(python3 - "$report" <<'EOF'
import json, sys
r = json.load(open(sys.argv[1]))
[s] = [s for s in r["stocks"] if s["symbol"] == "AAPL"]
pools = [p["pool"] for p in s["pools"]]
depths = [next(d["proceeds"] for d in p["depth"] if d["impact_bps"] == 500) for p in s["pools"]]
print(f'export FORK_BLOCK={r["block"]}')
print(f'export AAPL_POOLS={",".join(pools)}')
print(f'export AAPL_DEPTHS={",".join(depths)}')
EOF
)"
export FORK_RPC_URL="$rpc"
echo "fork block $FORK_BLOCK, AAPL pools $AAPL_POOLS, depths $AAPL_DEPTHS" >&2

cd "$root/contracts"
forge test --match-contract NetNetAaplForkTest -vv
