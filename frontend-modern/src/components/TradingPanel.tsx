import { useState } from 'react'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { DollarSign, TrendingUp, TrendingDown } from 'lucide-react'

interface TradingPanelProps {
  asset: { id: string; symbol: string; name: string }
  currentPrice: number
}

export function TradingPanel({ asset, currentPrice }: TradingPanelProps) {
  const [orderType, setOrderType] = useState<'buy' | 'sell'>('buy')
  const [amount, setAmount] = useState('')
  const [price, setPrice] = useState('')

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault()
    console.log(`Placing ${orderType} order:`, { asset: asset.symbol, amount, price })
    // TODO: Implement actual order placement
    alert(`${orderType.toUpperCase()} Order placed: ${amount} ${asset.symbol} @ $${price || currentPrice}`)
    setAmount('')
    setPrice('')
  }

  const total = parseFloat(amount || '0') * parseFloat(price || currentPrice.toString())

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-lg flex items-center gap-2">
          <DollarSign className="h-5 w-5" />
          Trading Panel
        </CardTitle>
      </CardHeader>
      <CardContent>
        <Tabs value={orderType} onValueChange={(v) => setOrderType(v as any)}>
          <TabsList className="w-full grid grid-cols-2">
            <TabsTrigger value="buy" className="data-[state=active]:bg-success data-[state=active]:text-white">
              <TrendingUp className="h-4 w-4 mr-2" />
              Buy
            </TabsTrigger>
            <TabsTrigger value="sell" className="data-[state=active]:bg-danger data-[state=active]:text-white">
              <TrendingDown className="h-4 w-4 mr-2" />
              Sell
            </TabsTrigger>
          </TabsList>

          <TabsContent value={orderType} className="mt-4">
            <form onSubmit={handleSubmit} className="space-y-4">
              <div>
                <label className="text-sm font-medium mb-2 block">
                  Amount ({asset.symbol})
                </label>
                <Input
                  type="number"
                  step="0.0001"
                  placeholder="0.00"
                  value={amount}
                  onChange={(e) => setAmount(e.target.value)}
                  required
                />
              </div>

              <div>
                <label className="text-sm font-medium mb-2 block">
                  Price (USD)
                </label>
                <div className="flex gap-2">
                  <Input
                    type="number"
                    step="0.01"
                    placeholder={currentPrice.toFixed(2)}
                    value={price}
                    onChange={(e) => setPrice(e.target.value)}
                  />
                  <Button
                    type="button"
                    variant="outline"
                    onClick={() => setPrice(currentPrice.toFixed(2))}
                  >
                    Market
                  </Button>
                </div>
                <p className="text-xs text-muted-foreground mt-1">
                  Leave empty for market price
                </p>
              </div>

              <div className="bg-muted rounded-lg p-3">
                <div className="flex justify-between mb-2">
                  <span className="text-sm text-muted-foreground">Total</span>
                  <span className="text-lg font-bold font-mono">
                    ${isNaN(total) ? '0.00' : total.toFixed(2)}
                  </span>
                </div>
                <div className="flex justify-between text-xs text-muted-foreground">
                  <span>Fee (0.1%)</span>
                  <span>${isNaN(total) ? '0.00' : (total * 0.001).toFixed(2)}</span>
                </div>
              </div>

              <Button
                type="submit"
                className="w-full"
                variant={orderType === 'buy' ? 'success' : 'destructive'}
                size="lg"
              >
                {orderType === 'buy' ? 'Buy' : 'Sell'} {asset.symbol}
              </Button>
            </form>
          </TabsContent>
        </Tabs>

        <div className="mt-4 pt-4 border-t border-border">
          <div className="grid grid-cols-2 gap-4 text-sm">
            <div>
              <div className="text-muted-foreground">Available Balance</div>
              <div className="font-semibold">$125,000.00</div>
            </div>
            <div>
              <div className="text-muted-foreground">{asset.symbol} Balance</div>
              <div className="font-semibold">2.5843</div>
            </div>
          </div>
        </div>
      </CardContent>
    </Card>
  )
}
