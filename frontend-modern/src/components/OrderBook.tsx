import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { TrendingDown, TrendingUp } from 'lucide-react'

interface OrderBookProps {
  bids: Array<{ price: number; size: number; total: number }>
  asks: Array<{ price: number; size: number; total: number }>
}

export function OrderBook({ bids, asks }: OrderBookProps) {
  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-lg">Order Book</CardTitle>
      </CardHeader>
      <CardContent>
        <div className="space-y-4">
          {/* Asks (Sell Orders) */}
          <div>
            <div className="flex items-center gap-2 mb-2 text-xs text-muted-foreground">
              <TrendingUp className="h-4 w-4 text-danger" />
              <span>Asks</span>
            </div>
            <div className="space-y-0.5">
              {asks.slice(0, 10).map((ask, idx) => (
                <div
                  key={idx}
                  className="grid grid-cols-3 gap-2 text-xs font-mono py-1 px-2 rounded hover:bg-accent transition-colors"
                >
                  <div className="text-danger font-semibold">{ask.price.toFixed(2)}</div>
                  <div className="text-right">{ask.size.toFixed(4)}</div>
                  <div className="text-right text-muted-foreground">{ask.total.toFixed(2)}</div>
                </div>
              ))}
            </div>
          </div>

          {/* Spread */}
          <div className="border-y border-border py-2">
            <div className="text-center">
              <div className="text-sm font-semibold">
                Spread: {bids.length && asks.length ? (asks[asks.length - 1].price - bids[0].price).toFixed(2) : '0.00'}
              </div>
            </div>
          </div>

          {/* Bids (Buy Orders) */}
          <div>
            <div className="flex items-center gap-2 mb-2 text-xs text-muted-foreground">
              <TrendingDown className="h-4 w-4 text-success" />
              <span>Bids</span>
            </div>
            <div className="space-y-0.5">
              {bids.slice(0, 10).map((bid, idx) => (
                <div
                  key={idx}
                  className="grid grid-cols-3 gap-2 text-xs font-mono py-1 px-2 rounded hover:bg-accent transition-colors"
                >
                  <div className="text-success font-semibold">{bid.price.toFixed(2)}</div>
                  <div className="text-right">{bid.size.toFixed(4)}</div>
                  <div className="text-right text-muted-foreground">{bid.total.toFixed(2)}</div>
                </div>
              ))}
            </div>
          </div>
        </div>
      </CardContent>
    </Card>
  )
}
