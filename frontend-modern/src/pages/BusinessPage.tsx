import { useCallback, useEffect, useMemo, useState } from 'react'
import { JsonPanel } from '@/components/JsonPanel'
import { Panel } from '@/components/Panel'
import { ApiError, asList, asRecord, createExchangeApi, type AuthConfig, type JsonRecord } from '@/services/exchangeApi'

interface PageProps {
  auth: AuthConfig
  onNotice: (message: string) => void
}

function parseNumber(value: string, fallback = 0): number {
  const parsed = Number(value)
  return Number.isFinite(parsed) ? parsed : fallback
}

function readText(value: unknown, fallback = '-'): string {
  if (typeof value === 'string' && value.length > 0) return value
  if (typeof value === 'number') return String(value)
  if (typeof value === 'boolean') return String(value)
  return fallback
}

export function BusinessPage({ auth, onNotice }: PageProps) {
  const api = useMemo(() => createExchangeApi(auth), [auth])
  const [loading, setLoading] = useState(false)
  const [marketId, setMarketId] = useState('btc-usdt')
  const [accountUser, setAccountUser] = useState(auth.subject)
  const [marketSummary, setMarketSummary] = useState<JsonRecord[]>([])
  const [rules, setRules] = useState<unknown>(null)
  const [ticker, setTicker] = useState<unknown>(null)
  const [klines, setKlines] = useState<JsonRecord[]>([])
  const [microstructure, setMicrostructure] = useState<unknown>(null)
  const [openInterest, setOpenInterest] = useState<unknown>(null)
  const [publicFundingRate, setPublicFundingRate] = useState<unknown>(null)
  const [markPrice, setMarkPrice] = useState<unknown>(null)
  const [book, setBook] = useState<unknown>(null)
  const [trades, setTrades] = useState<JsonRecord[]>([])
  const [balances, setBalances] = useState<JsonRecord[]>([])
  const [positions, setPositions] = useState<JsonRecord[]>([])
  const [margin, setMargin] = useState<unknown>(null)
  const [pnl, setPnl] = useState<unknown>(null)
  const [orders, setOrders] = useState<JsonRecord[]>([])
  const [fills, setFills] = useState<JsonRecord[]>([])
  const [ledger, setLedger] = useState<unknown>(null)
  const [withdrawals, setWithdrawals] = useState<JsonRecord[]>([])
  const [feeTier, setFeeTier] = useState<unknown>(null)
  const [feeSchedule, setFeeSchedule] = useState<JsonRecord[]>([])
  const [otcQuotes, setOtcQuotes] = useState<JsonRecord[]>([])
  const [earnPositions, setEarnPositions] = useState<JsonRecord[]>([])
  const [lastResponse, setLastResponse] = useState<unknown>(null)

  const [orderSide, setOrderSide] = useState('buy')
  const [orderType, setOrderType] = useState('limit')
  const [timeInForce, setTimeInForce] = useState('gtc')
  const [price, setPrice] = useState('50000')
  const [amount, setAmount] = useState('1000')
  const [outcome, setOutcome] = useState('0')
  const [clientOrderId, setClientOrderId] = useState('')

  const [cancelOrderId, setCancelOrderId] = useState('')
  const [replaceOrderId, setReplaceOrderId] = useState('')
  const [replacePrice, setReplacePrice] = useState('51000')
  const [replaceAmount, setReplaceAmount] = useState('1000')

  const [withdrawAmount, setWithdrawAmount] = useState('1000')
  const [withdrawAddress, setWithdrawAddress] = useState('0xabc123demo000000000000000000000000000000')

  const [otcQuoteMarket, setOtcQuoteMarket] = useState('otc:btc-usdt:block')
  const [otcQuoteSide, setOtcQuoteSide] = useState('buy')
  const [otcQuotePrice, setOtcQuotePrice] = useState('60000')
  const [otcQuoteAmount, setOtcQuoteAmount] = useState('10')
  const [otcAcceptQuoteId, setOtcAcceptQuoteId] = useState('')

  const [earnProductId, setEarnProductId] = useState('earn:usdc:flex')
  const [earnAmount, setEarnAmount] = useState('100000')

  useEffect(() => {
    setAccountUser(auth.subject)
  }, [auth.subject])

  const load = useCallback(async () => {
    setLoading(true)
    const numericOutcome = parseNumber(outcome)
    const results = await Promise.allSettled([
      api.getMarketsSummary(),
      api.getRules(),
      api.getTicker(marketId),
      api.getKlines(marketId, '1m', 32, numericOutcome),
      api.getMicrostructure(marketId),
      api.getOpenInterest(marketId),
      api.getPublicFundingRate(marketId),
      api.getMarkPrice(marketId),
      api.getBook(marketId, 20, numericOutcome),
      api.getTrades(marketId, 20, numericOutcome),
      api.getBalances(accountUser),
      api.getPositions(accountUser),
      api.getMargin(accountUser, marketId, numericOutcome),
      api.getPnl(accountUser, marketId, numericOutcome),
      api.getOrders(accountUser, marketId),
      api.getFills(accountUser, marketId),
      api.getLedger(accountUser),
      api.getWithdrawals(accountUser),
      api.getUserFeeTier(accountUser),
      api.getFeeTiers(),
      api.listOtcQuotes(),
      api.getEarnPositions(accountUser),
    ])

    const [
      summaryResult,
      rulesResult,
      tickerResult,
      klinesResult,
      microstructureResult,
      openInterestResult,
      fundingRateResult,
      markPriceResult,
      bookResult,
      tradesResult,
      balancesResult,
      positionsResult,
      marginResult,
      pnlResult,
      ordersResult,
      fillsResult,
      ledgerResult,
      withdrawalsResult,
      feeTierResult,
      feeScheduleResult,
      otcQuotesResult,
      earnPositionsResult,
    ] = results

    if (summaryResult.status === 'fulfilled') setMarketSummary(asList(summaryResult.value))
    if (rulesResult.status === 'fulfilled') setRules(rulesResult.value)
    if (tickerResult.status === 'fulfilled') setTicker(tickerResult.value)
    if (klinesResult.status === 'fulfilled') setKlines(asList(klinesResult.value, ['items', 'candles', 'klines']))
    if (microstructureResult.status === 'fulfilled') setMicrostructure(microstructureResult.value)
    if (openInterestResult.status === 'fulfilled') setOpenInterest(openInterestResult.value)
    if (fundingRateResult.status === 'fulfilled') setPublicFundingRate(fundingRateResult.value)
    if (markPriceResult.status === 'fulfilled') setMarkPrice(markPriceResult.value)
    if (bookResult.status === 'fulfilled') setBook(bookResult.value)
    if (tradesResult.status === 'fulfilled') setTrades(asList(tradesResult.value, ['items', 'trades']))
    if (balancesResult.status === 'fulfilled') setBalances(asList(balancesResult.value))
    if (positionsResult.status === 'fulfilled') setPositions(asList(positionsResult.value))
    if (marginResult.status === 'fulfilled') setMargin(marginResult.value)
    if (pnlResult.status === 'fulfilled') setPnl(pnlResult.value)
    if (ordersResult.status === 'fulfilled') setOrders(asList(ordersResult.value))
    if (fillsResult.status === 'fulfilled') setFills(asList(fillsResult.value))
    if (ledgerResult.status === 'fulfilled') setLedger(ledgerResult.value)
    if (withdrawalsResult.status === 'fulfilled') setWithdrawals(asList(withdrawalsResult.value))
    if (feeTierResult.status === 'fulfilled') setFeeTier(feeTierResult.value)
    if (feeScheduleResult.status === 'fulfilled') setFeeSchedule(asList(feeScheduleResult.value, ['items']))
    if (otcQuotesResult.status === 'fulfilled') setOtcQuotes(asList(otcQuotesResult.value))
    if (earnPositionsResult.status === 'fulfilled') setEarnPositions(asList(earnPositionsResult.value))

    const failedCount = results.filter((item) => item.status === 'rejected').length
    onNotice(failedCount > 0 ? `Business page refreshed with ${failedCount} backend call failures.` : 'Business page refreshed from live backend.')
    setLoading(false)
  }, [accountUser, api, marketId, onNotice, outcome])

  useEffect(() => {
    void load()
  }, [load])

  async function run(task: Promise<unknown>, successMessage: string) {
    try {
      const response = await task
      setLastResponse(response)
      onNotice(successMessage)
      await load()
    } catch (error) {
      const message = error instanceof ApiError ? `${error.message} (${error.status})` : error instanceof Error ? error.message : 'Business action failed'
      onNotice(message)
    }
  }

  const numericOutcome = parseNumber(outcome)
  const marketCards = marketSummary.slice(0, 8)
  const bookRecord = asRecord(book)
  const bids = Array.isArray(bookRecord.bids) ? (bookRecord.bids as unknown[]) : []
  const asks = Array.isArray(bookRecord.asks) ? (bookRecord.asks as unknown[]) : []

  return (
    <div className="page-grid">
      <section className="stat-grid">
        <div className="stat-card">
          <span>Selected market</span>
          <strong>{marketId}</strong>
        </div>
        <div className="stat-card">
          <span>Account user</span>
          <strong>{accountUser}</strong>
        </div>
        <div className="stat-card">
          <span>Open orders</span>
          <strong>{orders.length}</strong>
        </div>
        <div className="stat-card">
          <span>Recent fills</span>
          <strong>{fills.length}</strong>
        </div>
      </section>

      <Panel
        title="Business Terminal"
        subtitle="Order entry, market analytics, withdrawals, OTC, earn, and user projections from the current exchange backend."
        actions={
          <button type="button" className="button button-secondary" onClick={() => void load()}>
            {loading ? 'Refreshing...' : 'Refresh'}
          </button>
        }
      >
        <div className="form-grid form-grid-compact">
          <label className="field">
            <span>Market ID</span>
            <input value={marketId} onChange={(event) => setMarketId(event.target.value)} />
          </label>
          <label className="field">
            <span>Account User</span>
            <input value={accountUser} onChange={(event) => setAccountUser(event.target.value)} />
          </label>
          <label className="field">
            <span>Outcome</span>
            <input value={outcome} onChange={(event) => setOutcome(event.target.value)} />
          </label>
        </div>

        <div className="market-card-grid">
          {marketCards.map((item, index) => (
            <button
              key={`${readText(item.market_id, readText(item.id, 'market'))}-${index}`}
              type="button"
              className="market-card"
              onClick={() => {
                const nextMarket = readText(item.market_id, readText(item.id, marketId))
                setMarketId(nextMarket)
                onNotice(`Switched business view to ${nextMarket}.`)
              }}
            >
              <strong>{readText(item.market_id, readText(item.id, 'unknown-market'))}</strong>
              <span>{readText(item.state, readText(item.market_state, 'unknown'))}</span>
            </button>
          ))}
        </div>
      </Panel>

      <div className="two-column-grid">
        <Panel title="Order Ticket" subtitle="Submit, cancel, and replace trading orders.">
          <div className="form-grid">
            <label className="field">
              <span>Side</span>
              <select value={orderSide} onChange={(event) => setOrderSide(event.target.value)}>
                <option value="buy">buy</option>
                <option value="sell">sell</option>
              </select>
            </label>
            <label className="field">
              <span>Order Type</span>
              <select value={orderType} onChange={(event) => setOrderType(event.target.value)}>
                <option value="limit">limit</option>
                <option value="market">market</option>
              </select>
            </label>
            <label className="field">
              <span>Time In Force</span>
              <select value={timeInForce} onChange={(event) => setTimeInForce(event.target.value)}>
                <option value="gtc">gtc</option>
                <option value="ioc">ioc</option>
                <option value="fok">fok</option>
              </select>
            </label>
            <label className="field">
              <span>Price</span>
              <input value={price} onChange={(event) => setPrice(event.target.value)} disabled={orderType === 'market'} />
            </label>
            <label className="field">
              <span>Amount</span>
              <input value={amount} onChange={(event) => setAmount(event.target.value)} />
            </label>
            <label className="field field-span-2">
              <span>Client Order ID</span>
              <input value={clientOrderId} onChange={(event) => setClientOrderId(event.target.value)} placeholder="optional" />
            </label>
            <label className="field field-span-2">
              <span>Cancel Order ID</span>
              <input value={cancelOrderId} onChange={(event) => setCancelOrderId(event.target.value)} />
            </label>
            <label className="field field-span-2">
              <span>Replace Order ID</span>
              <input value={replaceOrderId} onChange={(event) => setReplaceOrderId(event.target.value)} />
            </label>
            <label className="field">
              <span>New Price</span>
              <input value={replacePrice} onChange={(event) => setReplacePrice(event.target.value)} />
            </label>
            <label className="field">
              <span>New Amount</span>
              <input value={replaceAmount} onChange={(event) => setReplaceAmount(event.target.value)} />
            </label>
          </div>
          <div className="button-row button-row-wrap">
            <button
              type="button"
              className="button button-primary"
              onClick={() =>
                void run(
                  api.submitOrder({
                    market_id: marketId,
                    side: orderSide,
                    order_type: orderType,
                    amount: parseNumber(amount),
                    outcome: numericOutcome,
                    time_in_force: timeInForce,
                    ...(clientOrderId.trim() ? { client_order_id: clientOrderId.trim() } : {}),
                    ...(orderType !== 'market' ? { price: parseNumber(price) } : {}),
                  }),
                  'Order submission accepted by backend.',
                )
              }
            >
              Submit Order
            </button>
            <button
              type="button"
              className="button button-secondary"
              onClick={() =>
                void run(
                  api.cancelOrder({
                    market_id: marketId,
                    order_id: cancelOrderId,
                    outcome: numericOutcome,
                  }),
                  'Cancel request accepted by backend.',
                )
              }
            >
              Cancel Order
            </button>
            <button
              type="button"
              className="button button-secondary"
              onClick={() =>
                void run(
                  api.replaceOrder({
                    market_id: marketId,
                    order_id: replaceOrderId,
                    outcome: numericOutcome,
                    new_price: parseNumber(replacePrice),
                    new_amount: parseNumber(replaceAmount),
                  }),
                  'Replace request accepted by backend.',
                )
              }
            >
              Replace Order
            </button>
          </div>
        </Panel>

        <Panel title="Market Analytics" subtitle="Ticker, mark price, funding, open interest, fee tier, and public rules.">
          <div className="three-column-grid">
            <div className="subcard">
              <h3>Ticker</h3>
              <div className="mini-list">
                <div className="mini-list-item">
                  <strong>Last</strong>
                  <span>{readText(asRecord(ticker).last_price, readText(asRecord(ticker).last))}</span>
                </div>
                <div className="mini-list-item">
                  <strong>24h Volume</strong>
                  <span>{readText(asRecord(ticker).volume_24h, readText(asRecord(ticker).volume))}</span>
                </div>
              </div>
            </div>
            <div className="subcard">
              <h3>Pricing</h3>
              <div className="mini-list">
                <div className="mini-list-item">
                  <strong>Mark Price</strong>
                  <span>{readText(asRecord(markPrice).mark_price, readText(asRecord(markPrice).price))}</span>
                </div>
                <div className="mini-list-item">
                  <strong>Funding</strong>
                  <span>{readText(asRecord(publicFundingRate).funding_rate_ppm, readText(asRecord(publicFundingRate).funding_rate))}</span>
                </div>
                <div className="mini-list-item">
                  <strong>Open Interest</strong>
                  <span>{readText(asRecord(openInterest).open_interest, readText(asRecord(openInterest).amount))}</span>
                </div>
              </div>
            </div>
            <div className="subcard">
              <h3>Fees And Rules</h3>
              <div className="mini-list">
                <div className="mini-list-item">
                  <strong>Tier</strong>
                  <span>{readText(asRecord(feeTier).tier)}</span>
                </div>
                <div className="mini-list-item">
                  <strong>Maker / Taker</strong>
                  <span>{readText(asRecord(feeTier).maker_fee_bps)}/{readText(asRecord(feeTier).taker_fee_bps)}</span>
                </div>
                <div className="mini-list-item">
                  <strong>Rule Count</strong>
                  <span>{Array.isArray(rules) ? rules.length : Object.keys(asRecord(rules)).length}</span>
                </div>
              </div>
            </div>
          </div>
        </Panel>
      </div>

      <div className="two-column-grid">
        <Panel title="Order Book" subtitle="Raw bids and asks from /markets/{market_id}/book.">
          <div className="table-wrap">
            <table className="data-table">
              <thead>
                <tr>
                  <th>Bid Price</th>
                  <th>Bid Amount</th>
                  <th>Ask Price</th>
                  <th>Ask Amount</th>
                </tr>
              </thead>
              <tbody>
                {Array.from({ length: Math.max(bids.length, asks.length, 8) }).map((_, index) => {
                  const bid = asRecord(bids[index])
                  const ask = asRecord(asks[index])
                  return (
                    <tr key={`book-${index}`}>
                      <td>{readText(bid.price)}</td>
                      <td>{readText(bid.amount)}</td>
                      <td>{readText(ask.price)}</td>
                      <td>{readText(ask.amount)}</td>
                    </tr>
                  )
                })}
              </tbody>
            </table>
          </div>
        </Panel>

        <Panel title="Recent Trades And Klines" subtitle="Executed trades plus the latest candle samples.">
          <div className="table-wrap">
            <table className="data-table">
              <thead>
                <tr>
                  <th>Trade/Candle</th>
                  <th>Price/Open</th>
                  <th>Amount/High</th>
                  <th>Time/Low</th>
                </tr>
              </thead>
              <tbody>
                {trades.slice(0, 6).map((item, index) => (
                  <tr key={`trade-${index}`}>
                    <td>{readText(item.trade_id, readText(item.id))}</td>
                    <td>{readText(item.price)}</td>
                    <td>{readText(item.amount, readText(item.quantity))}</td>
                    <td>{readText(item.recorded_at, readText(item.timestamp))}</td>
                  </tr>
                ))}
                {klines.slice(0, 6).map((item, index) => (
                  <tr key={`kline-${index}`}>
                    <td>{readText(item.ts, readText(item.time, 'kline'))}</td>
                    <td>{readText(item.open)}</td>
                    <td>{readText(item.high)}</td>
                    <td>{readText(item.low)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </Panel>
      </div>

      <div className="three-column-grid">
        <Panel title="Balances" subtitle="User cash and hold balances.">
          <div className="table-wrap compact-table">
            <table className="data-table">
              <thead>
                <tr>
                  <th>Asset</th>
                  <th>Available</th>
                  <th>Hold</th>
                </tr>
              </thead>
              <tbody>
                {balances.slice(0, 12).map((item, index) => (
                  <tr key={`bal-${index}`}>
                    <td>{readText(item.asset, readText(item.symbol))}</td>
                    <td>{readText(item.available)}</td>
                    <td>{readText(item.hold, readText(item.locked))}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </Panel>

        <Panel title="Positions And Risk" subtitle="Position size plus margin and PnL projections.">
          <div className="mini-list">
            {positions.slice(0, 6).map((item, index) => (
              <div key={`pos-${index}`} className="mini-list-item">
                <strong>{readText(item.market_id)}</strong>
                <span>size={readText(item.size, readText(item.position))} entry={readText(item.entry_price, readText(item.avg_entry_price))}</span>
              </div>
            ))}
            <div className="mini-list-item">
              <strong>Margin Snapshot</strong>
              <span>{readText(asRecord(margin).status, JSON.stringify(margin))}</span>
            </div>
            <div className="mini-list-item">
              <strong>PnL Snapshot</strong>
              <span>{readText(asRecord(pnl).status, JSON.stringify(pnl))}</span>
            </div>
          </div>
        </Panel>

        <Panel title="Open Orders And Fills" subtitle="Current order projection and trade fills.">
          <div className="mini-list">
            {orders.slice(0, 6).map((item, index) => (
              <div key={`ord-${index}`} className="mini-list-item">
                <strong>{readText(item.order_id, readText(item.id))}</strong>
                <span>{readText(item.side)} / {readText(item.status)}</span>
              </div>
            ))}
            {fills.slice(0, 4).map((item, index) => (
              <div key={`fill-${index}`} className="mini-list-item">
                <strong>{readText(item.trade_id, readText(item.id, 'fill'))}</strong>
                <span>{readText(item.price)} x {readText(item.amount)}</span>
              </div>
            ))}
          </div>
        </Panel>
      </div>

      <div className="two-column-grid">
        <Panel title="Withdrawals" subtitle="Create a withdrawal request and review recent withdrawal history.">
          <div className="form-grid">
            <label className="field">
              <span>Amount</span>
              <input value={withdrawAmount} onChange={(event) => setWithdrawAmount(event.target.value)} />
            </label>
            <label className="field field-span-2">
              <span>Destination Address</span>
              <input value={withdrawAddress} onChange={(event) => setWithdrawAddress(event.target.value)} />
            </label>
          </div>
          <div className="button-row">
            <button
              type="button"
              className="button button-primary"
              onClick={() =>
                void run(
                  api.requestWithdrawal({
                    amount: parseNumber(withdrawAmount),
                    destination_address: withdrawAddress,
                    asset: 'USDC',
                  }),
                  'Withdrawal request submitted.',
                )
              }
            >
              Request Withdrawal
            </button>
          </div>
          <div className="mini-list stacked-gap">
            {withdrawals.slice(0, 6).map((item, index) => (
              <div key={`wd-${index}`} className="mini-list-item">
                <strong>{readText(item.withdrawal_id, readText(item.id, 'withdrawal'))}</strong>
                <span>{readText(item.status)} / {readText(item.amount)} / {readText(item.destination_address)}</span>
              </div>
            ))}
          </div>
        </Panel>

        <Panel title="OTC And Earn" subtitle="Use the current OTC quote flow and earn subscription flow exposed by the backend.">
          <div className="form-grid">
            <label className="field field-span-2">
              <span>OTC Market</span>
              <input value={otcQuoteMarket} onChange={(event) => setOtcQuoteMarket(event.target.value)} />
            </label>
            <label className="field">
              <span>OTC Side</span>
              <select value={otcQuoteSide} onChange={(event) => setOtcQuoteSide(event.target.value)}>
                <option value="buy">buy</option>
                <option value="sell">sell</option>
              </select>
            </label>
            <label className="field">
              <span>OTC Price</span>
              <input value={otcQuotePrice} onChange={(event) => setOtcQuotePrice(event.target.value)} />
            </label>
            <label className="field">
              <span>OTC Amount</span>
              <input value={otcQuoteAmount} onChange={(event) => setOtcQuoteAmount(event.target.value)} />
            </label>
            <label className="field field-span-2">
              <span>Accept Quote ID</span>
              <input value={otcAcceptQuoteId} onChange={(event) => setOtcAcceptQuoteId(event.target.value)} />
            </label>
            <label className="field">
              <span>Earn Product</span>
              <input value={earnProductId} onChange={(event) => setEarnProductId(event.target.value)} />
            </label>
            <label className="field">
              <span>Earn Amount</span>
              <input value={earnAmount} onChange={(event) => setEarnAmount(event.target.value)} />
            </label>
          </div>
          <div className="button-row button-row-wrap">
            <button
              type="button"
              className="button button-primary"
              onClick={() =>
                void run(
                  api.createOtcQuote({
                    market_id: otcQuoteMarket,
                    side: otcQuoteSide,
                    price: parseNumber(otcQuotePrice),
                    amount: parseNumber(otcQuoteAmount),
                    outcome: 0,
                  }),
                  'OTC quote created.',
                )
              }
            >
              Create OTC Quote
            </button>
            <button
              type="button"
              className="button button-secondary"
              onClick={() => void run(api.acceptOtcQuote(otcAcceptQuoteId), 'OTC quote accepted.')}
            >
              Accept OTC Quote
            </button>
            <button
              type="button"
              className="button button-secondary"
              onClick={() =>
                void run(
                  api.subscribeEarn({
                    product_id: earnProductId,
                    amount: parseNumber(earnAmount),
                  }),
                  'Earn subscription submitted.',
                )
              }
            >
              Subscribe Earn
            </button>
            <button
              type="button"
              className="button button-secondary"
              onClick={() =>
                void run(
                  api.redeemEarn({
                    product_id: earnProductId,
                    amount: parseNumber(earnAmount),
                  }),
                  'Earn redemption submitted.',
                )
              }
            >
              Redeem Earn
            </button>
          </div>
          <div className="mini-list stacked-gap">
            {otcQuotes.slice(0, 4).map((item, index) => (
              <div key={`otc-${index}`} className="mini-list-item">
                <strong>{readText(item.quote_id)}</strong>
                <span>{readText(item.market_id)} / {readText(item.status)} / {readText(item.price)}</span>
              </div>
            ))}
            {earnPositions.slice(0, 4).map((item, index) => (
              <div key={`earn-${index}`} className="mini-list-item">
                <strong>{readText(item.product_id)}</strong>
                <span>{readText(item.principal_amount)} / apr {readText(item.apr_bps)}</span>
              </div>
            ))}
          </div>
        </Panel>
      </div>

      <div className="two-column-grid">
        <JsonPanel title="Ledger Projection" value={ledger ?? { info: 'No ledger payload loaded yet.' }} />
        <JsonPanel title="Microstructure" value={microstructure ?? { info: 'No microstructure payload loaded yet.' }} />
        <JsonPanel title="Rules And Fee Schedule" value={{ rules, feeSchedule, feeTier }} />
        <JsonPanel title="Last Backend Response" value={lastResponse ?? { info: 'No mutation submitted yet.' }} />
      </div>
    </div>
  )
}
