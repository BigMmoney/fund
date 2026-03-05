# 免费 API 集成方案

## 🔷 加密货币数据 API (免费)

### 1. **CoinGecko API** (当前使用)
- **免费额度**: 10-50 calls/min
- **URL**: https://api.coingecko.com/api/v3/
- **数据**: 价格、市值、24h变化、交易量
- **无需API Key**

### 2. **Binance Public API** (推荐)
- **免费额度**: 1200 requests/min  
- **URL**: https://api.binance.com/api/v3/
- **端点**:
  - 实时价格: `/api/v3/ticker/price?symbol=BTCUSDT`
  - 24h统计: `/api/v3/ticker/24hr?symbol=BTCUSDT`
  - K线数据: `/api/v3/klines?symbol=BTCUSDT&interval=1m`
  - 深度数据: `/api/v3/depth?symbol=BTCUSDT&limit=100`
- **无需API Key (公共端点)**

### 3. **Crypto Compare API**
- **免费额度**: 100,000 calls/month
- **URL**: https://min-api.cryptocompare.com/
- **数据**: 历史价格、OHLCV、社交数据
- **需要免费API Key**

### 4. **CoinCap API**
- **免费额度**: 200 requests/min
- **URL**: https://api.coincap.io/v2/
- **数据**: 实时价格、历史数据、交易所数据
- **无需API Key**

---

## 📈 股票数据 API (免费)

### 1. **Alpha Vantage** (推荐)
- **免费额度**: 25 requests/day (500 calls/day API Key)
- **URL**: https://www.alphavantage.co/query
- **端点**:
  - 实时报价: `?function=GLOBAL_QUOTE&symbol=IBM&apikey=YOUR_KEY`
  - 日内数据: `?function=TIME_SERIES_INTRADAY&symbol=IBM&interval=5min`
  - 每日数据: `?function=TIME_SERIES_DAILY&symbol=IBM`
- **免费API Key**: https://www.alphavantage.co/support/#api-key

### 2. **Yahoo Finance (非官方)**
- **免费**: 无限制
- **URL**: https://query1.finance.yahoo.com/
- **端点**:
  - 报价: `/v8/finance/chart/AAPL?interval=1m`
  - 历史: `/v7/finance/download/AAPL?period1=0&period2=9999999999&interval=1d`
- **无需API Key**
- **注意**: 非官方，可能不稳定

### 3. **Finnhub**
- **免费额度**: 60 calls/min
- **URL**: https://finnhub.io/api/v1/
- **端点**:
  - 实时报价: `/quote?symbol=AAPL&token=YOUR_TOKEN`
  - K线: `/stock/candle?symbol=AAPL&resolution=1&from=1572651390&to=1575243390`
- **免费API Key**: https://finnhub.io/register

### 4. **Twelve Data**
- **免费额度**: 800 calls/day
- **URL**: https://api.twelvedata.com/
- **数据**: 实时报价、技术指标、基本面数据
- **免费API Key**: https://twelvedata.com/pricing

---

## ⛓️ 区块链数据 API (免费)

### 1. **Etherscan API**
- **免费额度**: 5 calls/sec
- **URL**: https://api.etherscan.io/api
- **数据**: 
  - ETH余额查询
  - 交易历史
  - Gas价格
  - 智能合约数据
- **免费API Key**: https://etherscan.io/apis

### 2. **Blockchain.com API**
- **免费**: 无限制
- **URL**: https://blockchain.info/
- **端点**:
  - BTC价格: `/ticker`
  - 区块数据: `/block-height/$block_height?format=json`
  - 地址余额: `/balance?active=$address`
- **无需API Key**

### 3. **BlockCypher**
- **免费额度**: 200 requests/hour
- **URL**: https://api.blockcypher.com/v1/
- **支持**: BTC, ETH, LTC, DOGE
- **数据**: 链上数据、地址、交易
- **无需API Key (免费层)**

### 4. **Alchemy (Web3)**
- **免费额度**: 300M compute units/month
- **URL**: https://eth-mainnet.g.alchemy.com/v2/YOUR_KEY
- **功能**: 
  - 以太坊节点访问
  - NFT API
  - Webhooks
- **免费API Key**: https://www.alchemy.com/

---

## 🚀 推荐实现方案

### 方案 1: Binance + Alpha Vantage (最推荐)
```go
// 加密货币: Binance Public API (无需Key, 1200 req/min)
cryptoURL := "https://api.binance.com/api/v3/ticker/24hr"

// 股票: Alpha Vantage (免费Key, 500 req/day)
stockURL := "https://www.alphavantage.co/query?function=GLOBAL_QUOTE&symbol=IBM&apikey=YOUR_KEY"
```

**优点**:
- 高频率限制
- 稳定可靠
- 数据全面

### 方案 2: CoinGecko + Yahoo Finance (当前方案)
```go
// 加密货币: CoinGecko (当前使用)
cryptoURL := "https://api.coingecko.com/api/v3/coins/markets"

// 股票: Yahoo Finance (非官方)
stockURL := "https://query1.finance.yahoo.com/v8/finance/chart/AAPL"
```

**优点**:
- 完全免费
- 无需API Key
- 简单易用

**缺点**:
- CoinGecko限制较低 (10-50 calls/min)
- Yahoo非官方API

---

## 💡 代码实现示例

### Binance API 集成
```go
func fetchBinancePrices() ([]CryptoPrice, error) {
    symbols := []string{"BTCUSDT", "ETHUSDT", "SOLUSDT", "BNBUSDT"}
    url := "https://api.binance.com/api/v3/ticker/24hr"
    
    resp, err := http.Get(url)
    if err != nil {
        return nil, err
    }
    defer resp.Body.Close()
    
    var tickers []BinanceTicker
    json.NewDecoder(resp.Body).Decode(&tickers)
    
    // 转换为我们的格式
    var prices []CryptoPrice
    for _, ticker := range tickers {
        // 只处理我们关心的币种
        if contains(symbols, ticker.Symbol) {
            prices = append(prices, CryptoPrice{
                Symbol: ticker.Symbol,
                Price: parseFloat(ticker.LastPrice),
                Change24h: parseFloat(ticker.PriceChangePercent),
                Volume: parseFloat(ticker.Volume),
                High24h: parseFloat(ticker.HighPrice),
                Low24h: parseFloat(ticker.LowPrice),
            })
        }
    }
    
    return prices, nil
}
```

### Alpha Vantage API 集成
```go
func fetchAlphaVantagePrice(symbol string, apiKey string) (*StockPrice, error) {
    url := fmt.Sprintf(
        "https://www.alphavantage.co/query?function=GLOBAL_QUOTE&symbol=%s&apikey=%s",
        symbol, apiKey,
    )
    
    resp, err := http.Get(url)
    if err != nil {
        return nil, err
    }
    defer resp.Body.Close()
    
    var result AlphaVantageResponse
    json.NewDecoder(resp.Body).Decode(&result)
    
    quote := result.GlobalQuote
    return &StockPrice{
        Symbol: symbol,
        Price: parseFloat(quote.Price),
        Change: parseFloat(quote.Change),
        ChangePercent: parseFloat(quote.ChangePercent),
        Volume: parseInt(quote.Volume),
        High: parseFloat(quote.High),
        Low: parseFloat(quote.Low),
    }, nil
}
```

---

## 📊 数据更新频率建议

| 数据类型 | 推荐更新频率 | API选择 |
|---------|------------|---------|
| 加密货币实时价格 | 1-5秒 | Binance WebSocket |
| 加密货币概览 | 30秒 | Binance REST |
| 股票实时价格 | 15秒-1分钟 | Alpha Vantage |
| 历史K线数据 | 按需加载 | Binance/Alpha Vantage |
| 区块链数据 | 5-15秒 | Etherscan |
| 订单簿深度 | 1-3秒 | Binance WebSocket |

---

## 🔐 API Key 管理

创建配置文件 `config.yaml`:
```yaml
apis:
  alphavantage:
    key: "YOUR_ALPHA_VANTAGE_KEY"
    enabled: true
  
  finnhub:
    key: "YOUR_FINNHUB_KEY"
    enabled: false
  
  etherscan:
    key: "YOUR_ETHERSCAN_KEY"
    enabled: false
```

---

## ⚡ WebSocket 实时数据 (推荐用于HFT)

### Binance WebSocket
```javascript
// 前端实时连接
const ws = new WebSocket('wss://stream.binance.com:9443/ws/btcusdt@trade')

ws.onmessage = (event) => {
  const trade = JSON.parse(event.data)
  updatePrice(trade.s, trade.p) // symbol, price
}
```

### Finnhub WebSocket
```javascript
const socket = new WebSocket('wss://ws.finnhub.io?token=YOUR_TOKEN')

socket.addEventListener('open', () => {
  socket.send(JSON.stringify({'type':'subscribe', 'symbol': 'AAPL'}))
})

socket.addEventListener('message', (event) => {
  const data = JSON.parse(event.data)
  if (data.type === 'trade') {
    updateStockPrice(data.data)
  }
})
```

---

## 📝 总结

**最佳组合方案**:
1. **加密货币**: Binance API (REST + WebSocket)
2. **股票**: Alpha Vantage (REST) 或 Finnhub (WebSocket)
3. **区块链**: Etherscan API
4. **备用**: CoinGecko + Yahoo Finance

所有API都是免费的，只需注册获取API Key即可开始使用！
