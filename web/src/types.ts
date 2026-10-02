// Shape of web/public/snapshot.json, written by `egress-measure snapshot`.
// Every amount is an integer string in the token's own units, so nothing loses precision in JSON.

export type SnapPool = {
  address: string
  fee: number
  stock_is_token0: boolean
}

export type SnapVault = {
  address: string
  name: string
  kind: string
  /// The vault's supply assets in this market.
  allocation: string
  curator: string | null
  owner: string | null
}

export type SnapMarket = {
  id: string
  loan_symbol: string
  loan_decimals: number
  lltv_wad: string
  total_supplied: string
  total_borrowed: string
  collateral_units: string
  borrower_count: number
  vaults: SnapVault[]
  other_supplied: string
}

export type SnapStock = {
  symbol: string
  address: string
  decimals: number
  price_usdg: string | null
  pools: SnapPool[]
  sellable: string[]
  engine_sellable: string[]
  lent_units: string
  lent_usdg: string | null
  /// Present only on a composite file: depth measured in the same run as lent_usdg, so the two are
  /// comparable at one block. `sellable` above is from the newer depth run.
  sellable_at_lent_block?: string[]
  markets: SnapMarket[]
}

export type Source = {
  section: string
  file: string
  block: number
  timestamp: number
  taken_at_utc: string
}

export type LenderProtocol = {
  name: string
  source: string
  holders_checked: number
  value_usdg: string
  by_stock: Record<string, string>
}

export type Snapshot = {
  chain_id: number
  block: number
  timestamp: number
  taken_at_utc: string
  usdg: string
  usdg_decimals: number
  engine: string
  morpho_blue: string
  impacts_bps: number[]
  composite: boolean
  sources: Source[]
  totals: {
    lent_usdg: string
    sellable: string[]
    holders_checked: number
    protocols: number
    markets: number
  }
  stocks: SnapStock[]
  lenders: {
    holders_checked: number
    protocols: LenderProtocol[]
    not_covered: string[]
  }
  notes: string[]
}
