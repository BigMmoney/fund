# Prediction Platform - Dual-Rail (Offchain + Onchain)

A high-performance prediction market platform implementing FBA (Frequent Batch Auction) matching with dual-entry ledger accounting.

## Architecture

### Core Services

- **API Gateway** (`:8080`) - REST API and WebSocket endpoints
- **Ledger Service** (`:8081`) - Double-entry accounting system
- **Matching Engine** (`:8082`) - FBA batch auction processing
- **Indexer Service** (`:8083`) - Blockchain event monitoring
- **Risk Service** (`:8084`) - Market state management and risk controls

### Shared Components

- **Event Bus** - Inter-service communication
- **Types Package** - Shared data structures
- **Utils Package** - Common utilities

## Features

✅ **FBA Matching Engine**
- Batch auctions every 500ms
- Clearing price optimization
- Proportional fill allocation

✅ **Double-Entry Ledger**
- Atomic transactions
- Balance validation
- Operation idempotency
- Write-Ahead Log (WAL)

✅ **Blockchain Integration**
- Deposit/withdrawal tracking
- Reorg handling
- Confirmation requirements

✅ **Risk Management**
- Market state machine (OPEN/CLOSE_ONLY/CLOSED/FINALIZED)
- Dynamic risk adjustment
- Kill switch (L1-L4)

✅ **Real-time Updates**
- WebSocket support
- Event-driven architecture
- Live order book and trades

## Quick Start

### Prerequisites

- Go 1.21 or higher
- Git

### Installation

```powershell
# Clone repository
git clone <your-repo-url>
cd pre_trading

# Install dependencies
go mod tidy

# Start all services
.\start.ps1
```

### Manual Start (Individual Services)

```powershell
# Terminal 1 - Ledger
cd ledger
go run main.go

# Terminal 2 - Matching
cd matching
go run main.go

# Terminal 3 - Indexer
cd indexer
go run main.go

# Terminal 4 - Risk
cd risk
go run main.go

# Terminal 5 - API
cd api
go run main.go
```

## API Endpoints

### REST API

#### Create Intent
```bash
POST http://localhost:8080/v1/intents
Content-Type: application/json

{
  "user_id": "user1",
  "market_id": "market1",
  "side": "buy",
  "price": 55,
  "amount": 1000,
  "outcome": 0,
  "expires_in": 60
}
```

#### Get Markets
```bash
GET http://localhost:8080/v1/markets
```

#### Get Balances
```bash
GET http://localhost:8080/v1/balances?user_id=user1
```

#### Cancel Order
```bash
POST http://localhost:8080/v1/orders/{intent_id}/cancel
```

#### Request Withdrawal
```bash
POST http://localhost:8080/v1/withdrawals
Content-Type: application/json

{
  "user_id": "user1",
  "amount": 1000,
  "address": "0x..."
}
```

### WebSocket

```javascript
const ws = new WebSocket('ws://localhost:8080/ws');

ws.onmessage = (event) => {
  const data = JSON.parse(event.data);
  console.log('Event:', data);
};
```

## Configuration

Edit `config.yaml` to customize:

- Service ports
- Batch window duration
- Confirmation requirements
- Risk parameters
- Database connections

## Ledger Accounts

### User Accounts
- `U:{user_id}:USDC` - Available balance
- `U:{user_id}:USDC:HOLD` - Held balance for orders
- `U:{user_id}:OUTCOME:{market}:{outcome}` - Outcome shares

### Market Accounts
- `M:{market_id}:ESCROW:USDC` - Market escrow
- `M:{market_id}:FEE:USDC` - Collected fees
- `M:{market_id}:OUTCOME_POOL` - Outcome share pool

### System Accounts
- `SYS:ONCHAIN_VAULT:USDC` - Chain vault balance

## Market States

1. **PROPOSED** - Market created but not active
2. **OPEN** - Normal trading
3. **CLOSE_ONLY** - No new positions, can only close
4. **CLOSED** - No trading
5. **FINALIZED** - Resolved, settlements in progress

## Kill Switch Levels

- **L1** - Stop new positions
- **L2** - Stop withdrawals
- **L3** - Stop chain transactions
- **L4** - Read-only mode

## Development

### Project Structure

```
pre_trading/
├── api/              # API Gateway
├── ledger/           # Ledger Service
├── matching/         # Matching Engine
├── indexer/          # Indexer Service
├── risk/             # Risk Service
├── services/         # Shared packages
│   ├── types/        # Data structures
│   ├── eventbus/     # Event bus
│   └── utils/        # Utilities
├── frontend/         # React frontend (optional)
├── config.yaml       # Configuration
├── go.mod            # Go dependencies
├── start.ps1         # Startup script
└── README.md         # This file
```

### Adding a New Service

1. Create service directory
2. Implement main.go
3. Import shared types from `services/types`
4. Use event bus for communication
5. Add to start.ps1

## Testing

```powershell
# Run all tests
go test ./...

# Test specific service
cd ledger
go test -v

# Test with coverage
go test -cover ./...
```

## Monitoring

- Health check: `http://localhost:8080/health`
- Logs: `./logs/`
- Metrics: Configure Prometheus endpoint in config.yaml

## Production Deployment

### Prerequisites
- PostgreSQL database
- Redis cache
- Redpanda/Kafka event bus
- Blockchain RPC endpoint

### Configuration
1. Update `config.yaml` with production values
2. Set environment variables for secrets
3. Configure TLS certificates
4. Set up monitoring and alerting

### Security Checklist
- [ ] Change JWT secret
- [ ] Enable rate limiting
- [ ] Configure CORS properly
- [ ] Use API keys for service-to-service communication
- [ ] Enable HSM/KMS for signing
- [ ] Set up multi-sig for treasury
- [ ] Configure kill switch procedures
- [ ] Test backup and restore
- [ ] Run security audit

## Troubleshooting

### Services won't start
- Check if ports are available
- Verify Go is installed: `go version`
- Check logs in `./logs/`

### WebSocket not connecting
- Verify API service is running
- Check CORS configuration
- Ensure firewall allows connections

### Ledger transactions failing
- Check account balances
- Verify op_id uniqueness
- Review WAL logs

## Documentation

See `prediction_platform_full_design_v1_2.txt` for complete technical design.

## License

[Your License Here]

## Support

For issues and questions, please open a GitHub issue.
