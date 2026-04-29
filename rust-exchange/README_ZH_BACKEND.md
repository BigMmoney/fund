# Rust Exchange — 交易引擎

基于 Rust 构建的低延迟、高可靠加密货币/预测市场交易引擎。

---

## 一、项目概览

| 维度 | 说明 |
|------|------|
| **语言/运行时** | Rust 2021 Edition，Tokio 异步运行时 |
| **核心框架** | Warp HTTP 服务器 + Tokio broadcast 事件总线 |
| **监听端口** | `3030`（HTTP + WebSocket） |
| **认证机制** | HMAC-SHA256 内部签名认证 |
| **工作区结构** | 10 个 Crate（`crates/` 下），resolver = "2" |
| **构建方式** | `cargo build --release` / Docker 多阶段构建 |
| **数据持久化** | JSONL WAL（Write-Ahead Log），支持 CRC-32 校验与自动轮转 |

### 一句话架构

```
HTTP 网关 → 身份认证/权限 → 定序器 WAL → 风控检查 → 分片撮合引擎 → 复式账本提交 → 交易日志 → 快照/WAL 回放 → 读模型/风控自动化
```

---

## 二、架构总览

```
                      Client / Scripts / Frontend
                                 │
                                 ▼
                     ┌───────────────────────────┐
                     │  crates/api (API 网关)     │
                     │  • HTTP 路由               │
                     │  • HMAC 认证 / RBAC        │
                     │  • 限流 (IP/User/Admin)    │
                     │  • WebSocket 推送           │
                     └─────────────┬─────────────┘
                                   │ Command
                                   ▼
                     ┌───────────────────────────┐
                     │  crates/sequencer (定序器) │
                     │  • 单调递增 command_seq    │
                     │  • request_id 去重          │
                     │  • WAL 恢复 + 间隙检测      │
                     └─────────────┬─────────────┘
                                   │ Sequenced Command
                                   ▼
                     ┌───────────────────────────┐
                     │  crates/risk (风控引擎)    │
                     │  • 保证金/杠杆检查          │
                     │  • 强平 + ADL              │
                     │  • 资金费率计算             │
                     └─────────────┬─────────────┘
                                   │ Risk OK
                                   ▼
                     ┌───────────────────────────┐
                     │  crates/matching (撮合)    │
                     │  • 按 market_id+outcome 分片│
                     │  • 价格优先/时间优先        │
                     │  • Limit/Market/Stop/IOC/   │
                     │    FOK/GTD + STP           │
                     └─────────────┬─────────────┘
                                   │ Fill Events
                                   ▼
                     ┌───────────────────────────┐
                     │  crates/ledger (账本)      │
                     │  • 复式记账                │
                     │  • 64 分片锁               │
                     │  • 全局余额不变量验证       │
                     │  • CRC-32 WAL 持久化       │
                     └─────────────┬─────────────┘
                                   │ Committed Deltas
                                   ▼
                     ┌───────────────────────────┐
                     │  EventBus → WebSocket     │
                     │  实时推送成交/账本/风控事件  │
                     └───────────────────────────┘
```

### Crate 职责矩阵

| Crate | 职责 | 关键模块 |
|-------|------|----------|
| [`types`](crates/types/src/lib.rs) | 领域类型定义 | `Account`, `Order`, `Command`, `InstrumentSpec`, `Side`, `OrderType` |
| [`eventbus`](crates/eventbus/src/lib.rs) | Tokio broadcast 事件分发 | `IntentReceived`, `FillCreated`, `LedgerCommitted` |
| [`instruments`](crates/instruments/src/lib.rs) | 品种注册表 | Spot/Margin/Perp/Future/Option 规格管理 |
| [`sequencer`](crates/sequencer/src/lib.rs) | 命令定序 | 单调序列号、去重、WAL 恢复 |
| [`persistence`](crates/persistence/src/lib.rs) | WAL 实现 | 内存 WAL + JSONL 文件 WAL |
| [`ledger`](crates/ledger/src/lib.rs) | 复式账本 | op_id 幂等、64 分片、余额不变量 |
| [`risk`](crates/risk/src/lib.rs) | 风控引擎 | 保证金/强平/ADL/资金费率 |
| [`matching`](crates/matching/src/lib.rs) | 分片撮合引擎 | BTreeMap 订单簿、自成交防护、熔断器 |
| [`projections`](crates/projections/src/lib.rs) | 仓位/PnL 投影 | 保证金计算、未实现盈亏 |
| [`api`](crates/api/src/main.rs) | HTTP/WS 网关 | 路由、认证、WebSocket Hub、Prometheus |

---

## 三、配置文件

### 主配置：`config/exchange.toml`

**路径说明**：相对于 `rust-exchange/` 根目录，也可通过 `$EXCHANGE_CONFIG_PATH` 环境变量覆盖。

**加载优先级**：环境变量 > `config/exchange.toml` > 硬编码默认值。

```toml
[server]
bind_host = "127.0.0.1"
bind_port = 3030
log_level = "info"
max_body_size_bytes  = 16384
request_timeout_secs = 30

[wal]
data_dir             = "data"
rotation_max_entries = 100000
group_commit_size    = 64
ledger               = "data/ledger.wal.jsonl"
sequencer            = "data/sequencer.wal.jsonl"
matching_snapshot    = "data/matching.snapshot.jsonl"
trade_journal        = "data/trade_journal.wal.jsonl"
# ... 更多 WAL 路径

[risk]
automation_enabled               = false
liquidation_interval_secs        = 30
funding_interval_secs            = 60
maintenance_margin_bps           = 1000
liquidation_penalty_bps          = 500

[cors]
allowed_origins = ["http://127.0.0.1:5173", "http://localhost:5173"]

[websocket]
orderbook_snapshot_interval_ms = 200
max_connections                = 1024
```

### 环境变量：`.env`

**路径**：`rust-exchange/.env`（从 `.env.example` 复制）。

```bash
# HMAC 签名密钥（生产环境必须替换）
INTERNAL_AUTH_SHARED_SECRET=your-32-byte-secret-here

# 日志级别（tracing 框架）
RUST_LOG=info
```

**用途**：敏感配置不进入版本控制，供 `docker-compose up` 或本地开发读取。

**解析方法**：Docker Compose 通过 `${VAR}` 语法注入；本地开发可手动 `export` 或使用 PowerShell `$env:` 设置。

---

## 四、数据文件（WAL / JSONL）

所有运行时数据存储在 `data/` 目录下，采用 **JSONL 格式**（每行一条 JSON 记录），部分文件带 CRC-32 校验前缀。

### WAL 文件清单

| 文件 | 用途 | 写入方 | 示例条目数 |
|------|------|--------|-----------|
| [`data/ledger.wal.jsonl`](data/ledger.wal.jsonl) | 复式账本变动 | Ledger Service | 启动时种子存款 ~3 条 |
| [`data/sequencer.wal.jsonl`](data/sequencer.wal.jsonl) | 定序命令记录 | Sequencer | 运行后填充 |
| [`data/matching.snapshot.jsonl`](data/matching.snapshot.jsonl) | 订单簿快照 | Matching Engine | 定期写入 |
| [`data/trade_journal.wal.jsonl`](data/trade_journal.wal.jsonl) | 成交日志 | Matching Engine | 每次成交 |
| [`data/trade_settlement.wal.jsonl`](data/trade_settlement.wal.jsonl) | 结算记录 | Ledger | 每次结算 |
| [`data/instruments.registry.jsonl`](data/instruments.registry.jsonl) | 品种定义 | 启动引导 | ~5 条（Spot/Margin/Perp/Future/Option） |
| [`data/funding_rates.jsonl`](data/funding_rates.jsonl) | 资金费率 | Risk Engine | 定期更新 |
| [`data/position.cost.state.jsonl`](data/position.cost.state.jsonl) | 持仓成本基准 | Position Cost Tracker | 持仓变化时 |
| [`data/liquidation.queue.jsonl`](data/liquidation.queue.jsonl) | 待强排队列 | Liquidation Worker | 触发强平时 |
| [`data/adl.governance.jsonl`](data/adl.governance.jsonl) | ADL 候选排名 | Risk Engine | 需要自动减仓时 |
| [`data/index.price.jsonl`](data/index.price.jsonl) | 外部指数价格 | 喂价模块 | 定期推送 |
| [`data/governance.actions.jsonl`](data/governance.actions.jsonl) | 治理操作 | Admin API | 双签审批时 |
| [`data/withdrawals.wal.jsonl`](data/withdrawals.wal.jsonl) | 提现请求 | Custody Module | 用户提现时 |
| [`data/address_whitelist.wal.jsonl`](data/address_whitelist.wal.jsonl) | 提现地址白名单 | Custody Module | 地址管理时 |
| [`data/transfers.wal.jsonl`](data/transfers.wal.jsonl) | 转账记录 | Transfer Module | 内部转账时 |
| [`data/stop_orders.wal.jsonl`](data/stop_orders.wal.jsonl) | 止损止盈单 | Stop Order Module | 条件单触发时 |

**轮转策略**：单个 WAL 文件达到 `rotation_max_entries`（默认 100,000 条）时自动轮转。

### 读取示例

```powershell
# 查看最近 5 条账本记录
Get-Content data\ledger.wal.jsonl -Tail 5 | ForEach-Object { $_ -split '\t' | Select-Object -Last 1 | ConvertFrom-Json | ConvertTo-Json }

# 统计品种注册表
(Get-Content data\instruments.registry.jsonl).Count
```

```bash
# Linux/macOS 查看 ledger WAL
tail -5 data/ledger.wal.jsonl | cut -f2 | jq .
```

### WAL 条目示例

**Ledger 存款记录**（`data/ledger.wal.jsonl`）：
```json
{
  "op_id": "seed-demo-trader-usdc",
  "entries": [{
    "debit_account": "SYS:ONCHAIN_VAULT:USDC",
    "credit_account": "U:trader:USDC",
    "amount": 1000000,
    "op_id": "deposit_trader",
    "timestamp": "2026-04-08T20:19:11.713490Z"
  }],
  "timestamp": "2026-04-08T20:19:11.713492700Z"
}
```

**品种定义**（`data/instruments.registry.jsonl`）：
```json
{
  "spec": {
    "instrument_id": "perp:btc-usdt",
    "kind": "perpetual",
    "quote_asset": "USDC",
    "margin_mode": "isolated",
    "max_leverage": 20,
    "tick_size": 1,
    "lot_size": 1,
    "maker_fee_bps": 1,
    "taker_fee_bps": 4,
    "status": "active"
  },
  "recorded_at": "2026-04-08T20:19:11.730729400Z"
}
```

---

## 五、构建与部署

### 本地开发

```powershell
cd rust-exchange
cargo build --release
.\target\release\api.exe
```

### Docker 部署

```powershell
# 方案 A：直接用环境变量（开发）
$env:INTERNAL_AUTH_SHARED_SECRET = "your-secret-here"

# 方案 B：生产更推荐，用文件挂载
$env:INTERNAL_AUTH_SHARED_SECRET_FILE = "/run/secrets/exchange/internal_auth.secret"
$env:SERVER_ROLE_MAPPING_FILE = "/app/config/role_mapping.json"

# 构建并启动
docker-compose up -d

# 健康检查 / 就绪检查
curl http://localhost:3030/health
curl http://localhost:3030/ready
```

**Dockerfile 结构**：两阶段构建（`rust:1.88-slim` → `debian:bookworm-slim`），产物约 15MB。

### 生产配置覆盖

```bash
# 通过环境变量覆盖 TOML 配置
export RUST_LOG=debug
export API_BIND_HOST=0.0.0.0
export API_BIND_PORT=3030
```

---

## 六、API 接口

### 认证

所有写请求（`POST`）需携带 HMAC-SHA256 签名头。读请求（`GET`）可通过内部信任头或 HMAC 认证。

| Header | 说明 |
|--------|------|
| `X-Internal-Auth-Subject` | 用户/主体 ID |
| `X-Internal-Auth-Role` | 角色（`user` / `admin` / `system`） |
| `X-Internal-Auth-Session-Id` | 会话 ID |
| `X-Internal-Auth-Timestamp` | Unix 毫秒时间戳（偏差 ≤ 30s） |
| `X-Internal-Auth-Signature` | `HMAC-SHA256(timestamp + "\n" + method + "\n" + path + "\n" + body_sha256, shared_secret)` 的 hex 编码 |
| `X-Internal-Auth-Body-Sha256` | 请求体的 SHA-256 hex（POST 必填） |
| `X-Internal-Auth-Nonce` | 随机字符串（防重放） |

### 端点分类

| 分类 | 路径示例 | 认证 |
|------|----------|------|
| **系统** | `GET /health`, `GET /ready`, `GET /metrics`, `GET /metrics/prometheus`, `GET /version` | 公开 |
| **行情** | `GET /markets`, `GET /orderbook/{market_id}`, `GET /trades/{market_id}` | 公开 |
| **交易** | `POST /submit-order`, `POST /cancel-order`, `POST /replace-order` | HMAC |
| **账户** | `GET /balances/{user_id}`, `GET /positions/{user_id}` | HMAC |
| **管理** | `POST /admin/instrument`, `POST /admin/funding-rate` | Admin HMAC |
| **治理** | `POST /governance/liquidation-policy` | 双签 Admin |
| **WebSocket** | `ws://localhost:3030/ws` | 握手时认证 |

### OpenAPI 规范

```bash
# 获取 OpenAPI 3.0 JSON
curl http://localhost:3030/openapi.json
```

### 调用示例（PowerShell）

```powershell
# 使用 Invoke-ExchangeRequestAs 辅助函数（见 scripts/test_lib.ps1）
Invoke-ExchangeRequestAs -UserId "trader" -Method POST -Path "/submit-order" -Body @{
    market_id = "btc-usdt"
    side           = "buy"
    order_type     = "limit"
    price          = 50000
    amount         = 1
    outcome        = 1
}
```

---

## 七、测试基础设施

### 测试脚本（`scripts/`）

| 脚本 | 用途 | 执行时间 |
|------|------|----------|
| [`quick_perf_test.ps1`](scripts/quick_perf_test.ps1) | 6 阶段完整性能测试 | ~5 分钟 |
| [`benchmark_suite.ps1`](scripts/benchmark_suite.ps1) | 多模式基准测试（并发扫描/Soak/热市场） | 可配置 |
| [`soak_test_v2.ps1`](scripts/soak_test_v2.ps1) | 长时间稳定性测试 | 10-30 分钟 |
| [`cancel_storm_test.ps1`](scripts/cancel_storm_test.ps1) | 撤单风暴韧性测试 | ~2 分钟 |
| [`e2e_trading_test.ps1`](scripts/e2e_trading_test.ps1) | 端到端交易流程验证 | ~1 分钟 |
| [`test_insufficient_funds.ps1`](scripts/test_insufficient_funds.ps1) | 余额不足边界场景 | ~30 秒 |
| [`test_wal_recovery.ps1`](scripts/test_wal_recovery.ps1) | WAL 恢复能力测试 | ~1 分钟 |
| [`test_restart_after_errors.ps1`](scripts/test_restart_after_errors.ps1) | 故障重启恢复 | ~2 分钟 |

### Rust 单元测试

```bash
# 运行全部测试
cargo test --workspace

# 运行特定 crate 测试
cargo test -p matching
cargo test -p ledger
cargo test -p sequencer
```

### Criterion 基准测试

```bash
# 撮合引擎基准测试
cargo bench --package matching

# 订单流水线基准测试
cargo bench --package api
```

### 快速性能测试

```powershell
cd rust-exchange
.\scripts\quick_perf_test.ps1
```

**6 个测试阶段**：
1. **端点延迟**：各只读端点 50 次请求，测量 P50/P55/P95/P99
2. **顺序下单**：100 笔连续订单，基线延迟
3. **并发下单**：1/2/5/10 worker 并发，观察扩展性
4. **全链路**：60 笔可撮合订单，验证完整流水线
5. **复杂市场模拟**：高波动/多市场/深度挂单场景
6. **Soak 测试**：3 分钟持续负载，检测内存泄漏/性能退化

---

## 八、日志与 Metrics

### 结构化日志

**框架**：`tracing` + `tracing-subscriber`（JSON 格式化）

**配置**：`RUST_LOG` 环境变量（`trace`/`debug`/`info`/`warn`/`error`）

```bash
# 启动时设置调试日志
$env:RUST_LOG = "debug"
.\target\release\api.exe
```

**输出示例**（stdout JSON）：
```json
{"timestamp":"2026-04-08T20:19:11.713490Z","level":"INFO","message":"Order accepted","user_id":"trader","order_id":"abc-123","market_id":"btc-usdt","seq":42}
```

### Prometheus Metrics

**端点**：
- `GET /metrics` — JSON 格式的指标快照
- `GET /metrics/prometheus` — Prometheus text 格式（`text/plain; version=0.4.0`）

**暴露指标**：

| 指标名 | 类型 | 说明 |
|--------|------|------|
| `exchange_orders_received_total` | Counter | 接收订单总数 |
| `exchange_orders_filled_total` | Counter | 成交订单总数 |
| `exchange_orders_rejected_total` | Counter | 拒绝订单总数 |
| `exchange_orders_cancelled_total` | Counter | 撤单总数 |
| `exchange_settlements_committed_total` | Counter | 结算提交总数 |
| `exchange_wal_appends_total` | Counter | WAL 追加总数 |
| `exchange_wal_errors_total` | Counter | WAL 错误总数 |
| `exchange_ws_connections_active` | Gauge | 活跃 WS 连接数 |
| `exchange_http_requests_total` | Counter | HTTP 请求总数 |
| `exchange_http_errors_total` | Counter | HTTP 错误总数 |
| `exchange_match_latency_us` | Histogram | 撮合延迟分布 |
| `exchange_wal_append_latency_us` | Histogram | WAL 追加延迟分布 |
| `exchange_risk_check_latency_us` | Histogram | 风控检查延迟 |
| `exchange_matching_core_latency_us` | Histogram | 撮合核心延迟 |
| `exchange_settlement_latency_us` | Histogram | 结算延迟 |
| `exchange_partition_fills_total` | Counter | 各分片成交数 |

### Grafana 仪表板

**路径**：[`deploy/grafana/exchange-dashboard.json`](deploy/grafana/exchange-dashboard.json)

**用途**：导入 Grafana 后可视化所有 Prometheus 指标，包含延迟热力图、QPS 趋势、错误率面板。

---

## 九、性能测试结果

### 最新测试摘要（2026-04-09）

| 阶段 | P50 | P55 | P95 | P99 | 样本量 | 失败率 |
|------|-----|-----|-----|-----|--------|--------|
| HealthCheck | 27.57ms | 27.94ms | 36.24ms | 37.24ms | 50 | 0% |
| MarketsList | 26.70ms | 26.94ms | 31.31ms | 32.18ms | 50 | 0% |
| OrderBookQuery | 3.38ms | 3.52ms | 6.09ms | 9.49ms | 50 | 0% |
| BalanceQuery | 3.67ms | 3.76ms | 4.97ms | 5.43ms | 50 | 0% |
| SequentialOrders | 31.00ms | 31.83ms | 50.13ms | 53.73ms | 100 | 0% |
| Concurrent-1 | 30.61ms | 31.51ms | 55.40ms | 63.68ms | 100 | 0% |
| Concurrent-5 | 46.04ms | 47.44ms | 71.41ms | 85.51ms | 100 | 0% |
| Concurrent-10 | 84.16ms | 85.25ms | 136.77ms | 161.34ms | 100 | 0% |
| FullPipeline | 29.18ms | 29.55ms | 51.89ms | 53.01ms | 60 | 0% |
| ComplexMarket | 32.00ms | 32.51ms | 53.57ms | 56.37ms | 140 | 0% |
| **SoakTest (3min)** | **30.18ms** | **31.27ms** | **52.46ms** | **54.82ms** | **1882** | **0%** |

### 关键结论

- **零退化**：Soak P50 (30.18ms) 与基线 (27-32ms) 一致，无内存泄漏信号
- **并发线性扩展**：1→5 worker 延迟增长可控；10 worker 时出现锁竞争迹象
- **查询 vs 写入**：读端点 3-5ms，写端点 30ms+，符合持久化开销预期
- **30 分钟 Soak 历史最佳**：36,040 笔订单 0 失败，P99 稳定 42ms

详细报告见 [`BENCHMARK_REPORT_2026-04-06.md`](BENCHMARK_REPORT_2026-04-06.md)。

---

## 十、核心代码片段

### 订单结构体（[`crates/types/src/lib.rs`](crates/types/src/lib.rs)）

```rust
/// 订单侧：买入（bid）或卖出（ask）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side { Buy, Sell }

/// 订单类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderType {
    Limit, Market, StopMarket, StopLimit, TakeProfitMarket, TakeProfitLimit,
}

/// 新订单命令
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewOrderCommand {
    pub request_id: String,
    pub user_id: String,
    pub market_id: String,
    pub side: Side,
    pub order_type: OrderType,
    pub price: i64,           // 最小价格单位
    pub amount: i64,          // 最小数量单位
    pub outcome: i32,         // 预测市场结果索引
    pub time_in_force: TimeInForce,
    pub expire_at: Option<DateTime<Utc>>,
    pub trigger_price: Option<i64>,
}
```

### 账本条目（[`crates/types/src/lib.rs`](crates/types/src/lib.rs)）

```rust
/// 复式记账条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerEntry {
    pub debit_account: String,   // 借记账户
    pub credit_account: String,  // 贷记账户
    pub amount: i64,             // 金额（最小单位，始终为正）
    pub op_id: String,           // 操作 ID（幂等键）
    pub timestamp: DateTime<Utc>,
}
```

### 品种规格（来自 WAL 实际数据）

```json
{
  "instrument_id": "perp:btc-usdt",
  "kind": "perpetual",
  "margin_mode": "isolated",
  "max_leverage": 20,
  "tick_size": 1,
  "lot_size": 1,
  "maker_fee_bps": 1,
  "taker_fee_bps": 4,
  "status": "active"
}
```

---

## 十一、部署架构

### Kubernetes（[`deploy/k8s/`](deploy/k8s/)）

- Deployment + Service 清单
- `EXCHANGE_CONFIG_PATH=/app/config/exchange.toml`
- Secret 以文件方式挂载到 `INTERNAL_AUTH_SHARED_SECRET_FILE`
- 可选角色映射文件 `SERVER_ROLE_MAPPING_FILE`
- 健康检查探针（`/health`）+ 就绪探针（`/ready`）+ startup probe
- 非 root、只读根文件系统、drop capabilities、`seccomp=RuntimeDefault`
- Ingress + TLS 模板（`exchange.example.com` / `exchange-tls`）
- WAL 备份 `CronJob`，可把 `/app/data` 打包上传到对象存储

### Docker Compose

```yaml
# 快速启动
services:
  exchange:
    ports: ["3030:3030"]
    volumes:
      - exchange-data:/app/data
      - ./config:/app/config:ro
      - ./secrets:/run/secrets/exchange:ro
    environment:
      RUST_LOG: "info"
      EXCHANGE_CONFIG_PATH: "/app/config/exchange.toml"
      INTERNAL_AUTH_SHARED_SECRET_FILE: "/run/secrets/exchange/internal_auth.secret"
      SERVER_ROLE_MAPPING_FILE: "/app/config/role_mapping.json"
```

**数据卷**：`exchange-data` 持久化所有 WAL 文件。

---

## 十二、快速开始

### 前置要求

- Rust 1.88+（`rustup install stable`）
- PowerShell 5.1+（Windows）或 Bash（Linux/macOS）
- （可选）Docker + Docker Compose

### 启动服务

```powershell
# 1. 克隆并进入目录
cd rust-exchange

# 2. 设置环境变量
$env:INTERNAL_AUTH_SHARED_SECRET = "dev-secret-change-me"

# 3. 编译
cargo build --release

# 4. 启动
.\target\release\api.exe

# 5. 验证
curl http://localhost:3030/health
```

### 运行测试

```powershell
# 单元测试
cargo test --workspace

# 性能测试（6 阶段）
.\scripts\quick_perf_test.ps1

# Soak 测试（30 分钟）
.\scripts\soak_test_v2.ps1
```

---

## 十三、目录结构速查

```
rust-exchange/
├── Cargo.toml                  # Workspace 定义
├── Cargo.lock                  # 锁定依赖版本
├── config/
│   └── exchange.toml           # 主配置文件
├── data/                       # 运行时 WAL 数据
│   ├── ledger.wal.jsonl
│   ├── sequencer.wal.jsonl
│   ├── matching.snapshot.jsonl
│   ├── instruments.registry.jsonl
│   └── ... (18 个 WAL 文件)
├── crates/
│   ├── types/                  # 领域类型
│   ├── eventbus/               # 事件总线
│   ├── instruments/            # 品种注册
│   ├── sequencer/              # 命令定序
│   ├── persistence/            # WAL 持久化
│   ├── ledger/                 # 复式账本
│   ├── risk/                   # 风控引擎
│   ├── matching/               # 撮合引擎
│   ├── projections/            # 仓位/PnL 投影
│   └── api/                    # HTTP/WS 网关 (main.rs)
│       ├── src/
│       │   ├── main.rs         # 入口 + 路由注册（~4700 行）
│       │   ├── config.rs       # 配置结构体
│       │   ├── observability.rs # Metrics 计数器/直方图
│       │   ├── prometheus.rs   # Prometheus 导出
│       │   ├── websocket.rs    # WebSocket Hub
│       │   ├── trading.rs      # 交易路由
│       │   ├── admin.rs        # 管理路由
│       │   └── ... (37 个模块)
│       └── benches/            # Criterion 基准测试
├── scripts/                    # 测试/基准脚本（~30 个 .ps1）
├── deploy/
│   ├── grafana/                # Grafana 仪表板
│   └── k8s/                    # Kubernetes 清单
├── Dockerfile                  # 多阶段构建
├── docker-compose.yml          # 容器编排
├── .env                        # 环境变量（不提交）
├── .env.example                # 环境变量模板
└── docs/                       # 架构文档
```

---

## 十四、依赖清单

### 核心依赖

| Crate | 版本 | 用途 |
|-------|------|------|
| tokio | 1.42 | 异步运行时 |
| warp | 0.3 | HTTP/WebSocket 框架 |
| serde | 1.0 | 序列化/反序列化 |
| serde_json | 1.0 | JSON 处理 |
| tracing | 0.1 | 结构化日志 |
| tracing-subscriber | 0.3 | 日志订阅者（JSON + env-filter） |
| dashmap | 6.1 | 并发哈希表 |
| parking_lot | 0.12 | 高性能锁 |
| chrono | 0.4 | 时间处理 |
| uuid | 1.11 | UUID 生成 |
| hmac | 0.12 | HMAC 认证 |
| sha2 | 0.10 | SHA-256 哈希 |
| toml | 0.8 | TOML 配置解析 |

### 开发依赖

| Crate | 版本 | 用途 |
|-------|------|------|
| criterion | 0.5 | 基准测试框架 |
| proptest | 1.5 | 属性测试 |
| tempfile | 3.14 | 临时文件（测试用） |

---

## 十五、安全与合规

- **认证**：HMAC-SHA256 签名，30 秒时间窗口防重放
- **授权**：RBAC（User / Admin / System 角色）
- **限流**：按 IP、用户、管理员三级令牌桶
- **治理**：关键操作需双签审批（Dual-Approval）
- **审计**：所有风控操作写入 `risk_automation.audit.jsonl`
- **WAL 完整性**：CRC-32 校验，启动时自动检测间隙

---

## 十六、相关文档

| 文档 | 说明 |
|------|------|
| [`ARCHITECTURE_ZH_RUNTIME_ALIGNMENT_2026-03-23.md`](ARCHITECTURE_ZH_RUNTIME_ALIGNMENT_2026-03-23.md) | 运行时对齐中文架构文档 |
| [`BENCHMARK_REPORT_2026-04-06.md`](BENCHMARK_REPORT_2026-04-06.md) | 综合性能测试报告 |
| [`DEEP_SECURITY_AUDIT_2026-04-07.md`](DEEP_SECURITY_AUDIT_2026-04-07.md) | 深度安全审计报告 |
| [`SECURITY.md`](SECURITY.md) | 安全策略 |
| [`TRADING_RISK_STATE_MACHINE_ZH_2026-03-12.md`](TRADING_RISK_STATE_MACHINE_ZH_2026-03-12.md) | 交易风控状态机 |
