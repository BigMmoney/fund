# Pre-Trading 真实代码链路与架构文档

> 生成日期：2026-04-06
> 基于版本：rust-exchange v0.1.0 (Workspace 10 crates)
> 状态：✅ 构建通过 · Release Profile 编译成功

---

## 一、系统概览

本系统是一个**基于 Rust 的高性能交易引擎**，采用 Workspace 多 crate 架构。核心运行时为单进程多线程模型，通过 WAL（Write-Ahead Log）实现所有状态的持久化和崩溃恢复。

### 技术栈

| 层级 | 技术选型 |
|------|----------|
| 语言 | Rust 2021 Edition |
| HTTP 框架 | warp 0.3 |
| 并发原语 | DashMap, parking_lot, tokio |
| 序列化 | serde + serde_json (JSONL) |
| 时间 | chrono |
| 日志 | tracing + tracing-subscriber (JSON) |
| 持久化 | 自定义 JSONL WAL (文件级 Write-Ahead Log) |

### 工作区结构（10 个 Crates）

```
rust-exchange/
├── crates/
│   ├── types/          ← 核心领域类型（订单、命令、事件、工具规格）
│   ├── instruments/    ← 工具注册表（合约定义、状态管理）
│   ├── eventbus/       ← 内存事件总线（跨组件消息传递）
│   ├── persistence/    ← JSONL WAL 实现（崩溃恢复基础）
│   ├── sequencer/      ← 命令排序器（全局因果序）
│   ├── ledger/         ← 账本服务（余额、结算）
│   ├── risk/           ← 风控引擎（保证金、清算、ADL）
│   ├── matching/       ← 撮合引擎（分区撮合 + 高性能撮合）
│   ├── projections/    ← 投影计算（PnL、仓位、保证金快照）
│   └── api/            ← API 层 + 启动引导（warp HTTP + WebSocket）
├── config/             ← 配置文件
├── data/               ← WAL 数据目录（运行时生成）
├── tests/              ← 集成测试
└── deploy/             ← 部署配置
```

---

## 二、真实代码链路（端到端数据流）

### 2.1 完整订单生命周期

```
客户端 HTTP POST /api/v1/intent
        │
        ▼
┌─────────────────────────────────────────┐
│ 1. API 层 (crates/api/src/trading.rs)   │
│   - 认证鉴权 (JWT/HMAC)                 │
│   - IP + User 速率限制                  │
│   - 输入校验 (market_id, amount, price) │
│   - 系统状态检查 (drain/kill-switch)    │
└─────────────────────────────────────────┘
        │
        ▼
┌─────────────────────────────────────────┐
│ 2. 排序器 (crates/sequencer/src/lib.rs)  │
│   - 分配全局单调递增序列号 (seq_no)     │
│   - 写入 Sequencer WAL (持久化)         │
│   - 保证全局因果序                       │
└─────────────────────────────────────────┘
        │
        ▼
┌─────────────────────────────────────────┐
│ 3. 风控引擎 (crates/risk/src/lib.rs)     │
│   - 检查用户余额是否充足                 │
│   - 检查保证金要求 (初始/维持保证金)    │
│   - Reduce-Only 校验                    │
│   - 杠杆与持仓限制                      │
│   - 产出 RiskCheckedCommand             │
└─────────────────────────────────────────┘
        │
        ▼
┌─────────────────────────────────────────┐
│ 4. 撮合引擎 (crates/matching/)           │
│   ├── partitioned/  分区撮合引擎         │
│   │   - 按市场分区并行处理              │
│   │   - 价格/时间优先撮合               │
│   │   - 自成交防护 (STP)                │
│   │   - 产出 Fill 事件                  │
│   └── high_performance/ 高性能路径      │
│       - 无锁数据结构优化                │
│       - 批量处理                        │
└─────────────────────────────────────────┘
        │
        ▼
┌─────────────────────────────────────────┐
│ 5. 账本服务 (crates/ledger/src/lib.rs)   │
│   - 根据 Fill 更新买卖双方余额          │
│   - 手续费扣除                          │
│   - 写入 Ledger WAL (持久化)            │
│   - 维护全局不变量 (资产守恒)           │
└─────────────────────────────────────────┘
        │
        ▼
┌─────────────────────────────────────────┐
│ 6. 持久化层 (crates/persistence/)        │
│   - Trade Journal WAL (成交记录)        │
│   - Trade Settlement WAL (结算记录)     │
│   - Matching Snapshot WAL (快照)        │
│   - 自动轮转 + Group Commit             │
└─────────────────────────────────────────┘
        │
        ▼
┌─────────────────────────────────────────┐
│ 7. 事件总线 (crates/eventbus/src/lib.rs) │
│   - 发布 Fill/OrderUpdate 事件          │
│   - WebSocket 推送给订阅客户端          │
│   - 自动化任务监听 (清算/资金费率)      │
└─────────────────────────────────────────┘
```

### 2.2 关键代码路径追踪

#### 下单接口入口

```rust
// crates/api/src/main.rs → build_trading_routes()
// 路由: POST /api/v1/intent
POST /api/v1/intent
  → validate_order_fields()        // 输入校验
  → ip_rate_limiter.check()        // IP 限频 (60 req/window)
  → user_rate_limiter.check()      // 用户限频 (30 req/window)
  → sentinel::enforce_order_posture() // 系统姿态检查
  → sequence_new_order()           // 排序器分配 seq_no
  → risk.check_order()             // 风控检查
  → partitioned_engine.submit()    // 提交撮合
  → emit_fill_events()             // 事件广播
```

#### 风控检查流程

```rust
// crates/risk/src/lib.rs → RiskEngine::check_order()
fn check_order(&self, cmd: &NewOrderCommand) -> Result<RiskCheckedCommand, RiskError> {
    // 1. 获取用户账户信息
    let account = self.ledger.get_account(&cmd.user_id)?;
    
    // 2. Reduce-Only 校验（不能增加反向持仓）
    if cmd.reduce_only && !self.can_reduce_position(cmd) {
        return Err(RiskError::InsufficientReduceOnlyPosition);
    }
    
    // 3. 保证金检查
    let required_margin = calculate_initial_margin(cmd);
    if account.available_balance < required_margin {
        return Err(RiskError::InsufficientMargin);
    }
    
    // 4. 产出已通过风控的指令
    Ok(RiskCheckedCommand { ... })
}
```

#### 撮合流程

```rust
// crates/matching/src/partitioned.rs
// 分区撮合引擎：每个市场独立分区，可并行处理
fn submit_order(&self, cmd: RiskCheckedCommand) -> SubmitOrderResult {
    // 1. 获取或创建市场分区
    let partition = self.get_partition(&cmd.market_id);
    
    // 2. 价格/时间优先撮合
    let fills = partition.match_order(&cmd);
    
    // 3. 自成交防护
    let filtered_fills = self.apply_stp(fills, &cmd.user_id);
    
    // 4. 记录成交到 Trade Journal WAL
    for fill in &filtered_fills {
        self.trade_journal_wal.append(fill)?;
    }
    
    // 5. 更新订单簿快照
    self.update_snapshot(&cmd.market_id);
    
    SubmitOrderResult { fills, resting_order }
}
```

---

## 三、启动引导流程（Bootstrap）

```rust
// crates/api/src/bootstrap.rs → bootstrap_runtime()
async fn bootstrap_runtime(event_bus: EventBus) -> AppBootstrap {
    // ═══ 阶段 1: WAL 初始化 ═══
    // 创建数据目录，初始化所有 WAL 文件
    let ledger_wal = JsonlFileWal::with_rotation(&ledger_wal_path, rotation_max);
    let sequencer_wal = JsonlFileWal::with_rotation(&sequencer_wal_path, rotation_max);
    let matching_snapshot_wal = JsonlFileWal::with_rotation(...);
    let trade_journal_wal = JsonlFileWal::with_rotation(...);
    let trade_settlement_wal = JsonlFileWal::with_rotation(...);
    
    // ═══ 阶段 2: 账本恢复 ═══
    let ledger = LedgerService::with_wal_store(event_bus.clone(), ledger_wal);
    ledger.recover_from_wal()?;           // 从 WAL 重放恢复状态
    seed_demo_balances(&ledger);          // 注入演示余额
    seed_demo_inventory(&ledger);         // 注入演示库存
    
    // ═══ 阶段 3: 排序器恢复 ═══
    let sequencer = Sequencer::with_wal(1, sequencer_wal);
    sequencer.recover_from_wal()?;        // 恢复序列号计数器
    
    // ═══ 阶段 4: 撮合引擎初始化 ═══
    let partitioned_engine = PartitionedMatchingEngine::with_stores(...);
    
    // ═══ 阶段 5: 命令重放 ═══
    // 从快照后的 WAL 条目重放，确保状态一致
    replay_commands_after_snapshot(&partitioned_engine, &sequencer).await?;
    
    // ═══ 阶段 6: 位置成本同步 ═══
    position_costs.sync_from_trade_journal(trade_journal_wal)?;
    
    // ═══ 阶段 7: 启动自检 ═══
    ledger.verify_global_invariant()?;    // 验证资产守恒
    
    AppBootstrap { ledger, sequencer, risk, instruments, partitioned_engine, ... }
}
```

### 启动时序图

```
时间线 →
┌─────────────┬──────────────────────────────────────────────┐
│ WAL Init    │ 创建 15+ 个 JSONL WAL 文件                   │
├─────────────┼──────────────────────────────────────────────┤
│ Ledger      │ 重放 WAL → 恢复余额 → 注入演示数据           │
├─────────────┼──────────────────────────────────────────────┤
│ Sequencer   │ 重放 WAL → 恢复 seq_no 计数器                │
├─────────────┼──────────────────────────────────────────────┤
│ Matching    │ 加载快照 → 重建订单簿状态                    │
├─────────────┼──────────────────────────────────────────────┤
│ Replay      │ 重放快照后的未处理指令                       │
├─────────────┼──────────────────────────────────────────────┤
│ Self-Test   │ 验证账本全局不变量                           │
├─────────────┼──────────────────────────────────────────────┤
│ HTTP Server │ 启动 warp 服务器 (默认 0.0.0.0:3030)         │
├─────────────┼──────────────────────────────────────────────┤
│ WebSocket   │ 启动 WS Hub (实时行情推送)                   │
├─────────────┼──────────────────────────────────────────────┤
│ Automation  │ 启动后台任务 (清算/资金费率/成本同步)        │
└─────────────┴──────────────────────────────────────────────┘
```

---

## 四、WAL 持久化体系

### 4.1 WAL 文件清单

| WAL 文件 | 默认路径 | 记录类型 | 用途 |
|----------|----------|----------|------|
| `ledger.wal.jsonl` | `data/ledger.wal.jsonl` | `LedgerDelta` | 账本变更流水 |
| `sequencer.wal.jsonl` | `data/sequencer.wal.jsonl` | `SequencedCommandRecord` | 排序指令流水 |
| `matching.snapshot.jsonl` | `data/matching.snapshot.jsonl` | `PartitionSnapshotRecord` | 撮合引擎快照 |
| `trade_journal.wal.jsonl` | `data/trade_journal.wal.jsonl` | `TradeJournalRecord` | 成交记录 |
| `trade_settlement.wal.jsonl` | `data/trade_settlement.wal.jsonl` | `TradeSettlementRecord` | 结算记录 |
| `instruments.registry.jsonl` | `data/instruments.registry.jsonl` | `InstrumentSpec` | 合约注册表 |
| `funding_rates.jsonl` | `data/funding_rates.jsonl` | `FundingRateRecord` | 资金费率历史 |
| `liquidation.queue.jsonl` | `data/liquidation.queue.jsonl` | `LiquidationQueueEntry` | 清算队列 |
| `liquidation.auction.jsonl` | `data/liquidation.auction.jsonl` | `LiquidationAuctionRecord` | 清算拍卖 |
| `adl.governance.jsonl` | `data/adl.governance.jsonl` | `AdlGovernanceRecord` | ADL 治理 |
| `index.price.jsonl` | `data/index.price.jsonl` | `IndexPriceRecord` | 指数价格 |
| `position.cost.state.jsonl` | `data/position.cost.state.jsonl` | `PositionCostState` | 持仓成本状态 |
| `position.cost.events.jsonl` | `data/position.cost.events.jsonl` | `PositionCostEvent` | 持仓成本事件 |
| `governance.actions.jsonl` | `data/governance.actions.jsonl` | `GovernanceAction` | 治理操作 |
| `replay_guard.jsonl` | `data/replay_guard.jsonl` | `ReplayGuardRecord` | 重放保护 |
| `risk_automation.audit.jsonl` | `data/risk_automation.audit.jsonl` | `RiskAutomationAuditEntry` | 风控审计 |

### 4.2 WAL 实现原理

```rust
// crates/persistence/src/lib.rs
pub struct JsonlFileWal<T> {
    path: PathBuf,
    file: File,
    rotation_max_entries: u64,    // 轮转阈值（0 = 禁用）
    group_commit_size: u64,       // Group Commit 大小（0 = 每次 sync）
    pending_count: u64,           // 当前待提交计数
}

impl<T: Serialize> WalStore<T> for JsonlFileWal<T> {
    fn append(&mut self, record: &T) -> Result<()> {
        // 1. 序列化为 JSON 行
        let line = serde_json::to_string(record)?;
        writeln!(self.file, "{}", line)?;
        
        // 2. Group Commit：累积到阈值才 flush
        self.pending_count += 1;
        if self.group_commit_size == 0 || self.pending_count >= self.group_commit_size {
            self.file.sync_all()?;
            self.pending_count = 0;
        }
        
        // 3. 轮转检查
        if self.rotation_max_entries > 0 && self.entry_count >= self.rotation_max_entries {
            self.rotate()?;
        }
        
        Ok(())
    }
}
```

---

## 五、风控子系统

### 5.1 风控引擎架构

```rust
// crates/risk/src/lib.rs
pub struct RiskEngine {
    ledger: Arc<LedgerService>,              // 账本查询
    collateral_table: Vec<CollateralAsset>,  // 抵押品表（多币种）
    user_risk_limits: Arc<RwLock<HashMap<String, UserRiskLimits>>>, // 用户限额
}
```

### 5.2 保证金计算

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `maintenance_margin_bps` | 50 (0.5%) | 维持保证金比例 |
| `liquidation_penalty_bps` | 100 (1%) | 清算惩罚 |
| 预警级别 Warning | 120% | 保证金比率 ≤ 120% 触发警告 |
| 预警级别 Critical | 105% | 保证金比率 ≤ 105% 即将清算 |

### 5.3 清算流程

```
持仓监控 → 保证金比率检查 → 触发清算
    │
    ▼
┌──────────────────────────────────────┐
│ LiquidationGate (速度限制器)         │
│ - 防止清算风暴                       │
│ - Circuit Breaker 机制              │
└──────────────────────────────────────┘
    │
    ▼
┌──────────────────────────────────────┐
│ 清算拍卖 (Auction Window)            │
│ - 默认窗口期可配置                   │
│ - 做市商/清算者竞价                  │
└──────────────────────────────────────┘
    │
    ▼
┌──────────────────────────────────────┐
│ ADL (自动减仓)                       │
│ - 拍卖失败后触发                     │
│ - 盈利对手方按优先级减仓             │
└──────────────────────────────────────┘
```

---

## 六、API 端点全景

### 6.1 REST API

| 方法 | 路径 | 功能 | 认证 |
|------|------|------|------|
| POST | `/api/v1/intent` | 下单（意图提交） | ✅ JWT/HMAC |
| DELETE | `/api/v1/orders/{order_id}` | 撤单 | ✅ |
| PUT | `/api/v1/orders/{order_id}` | 改单 | ✅ |
| GET | `/api/v1/orders` | 查询活跃订单 | ✅ |
| GET | `/api/v1/balance` | 查询余额 | ✅ |
| GET | `/api/v1/positions` | 查询持仓 | ✅ |
| GET | `/api/v1/markets` | 市场列表 | ❌ |
| GET | `/api/v1/orderbook/{market_id}` | 订单簿深度 | ❌ |
| GET | `/api/v1/trades/{market_id}` | 最近成交 | ❌ |
| GET | `/api/v1/funding-rates/{market_id}` | 资金费率 | ❌ |
| GET | `/api/v1/candlesticks/{market_id}` | K 线数据 | ❌ |
| POST | `/api/v1/admin/actions` | 管理员操作 | ✅ Admin |
| POST | `/api/v1/transfers` | 转账 | ✅ |
| POST | `/api/v1/withdrawals` | 提现 | ✅ |

### 6.2 WebSocket

| 事件类型 | 订阅路径 | 说明 |
|----------|----------|------|
| `orderbook_update` | `ws://host/ws` | 订单簿实时更新 |
| `trade_fill` | `ws://host/ws` | 成交回报 |
| `account_update` | `ws://host/ws` | 账户变更 |
| `position_update` | `ws://host/ws` | 持仓变更 |

### 6.3 管理端点

| 方法 | 路径 | 功能 |
|------|------|------|
| POST | `/api/v1/admin/kill-switch` | 紧急停机 |
| POST | `/api/v1/admin/resume` | 恢复服务 |
| POST | `/api/v1/admin/drain` | 优雅下线（停止接单） |
| GET | `/api/v1/admin/system-status` | 系统状态 |
| GET | `/health` | 健康检查 |
| GET | `/metrics` | Prometheus 指标 |

---

## 七、配置体系

### 7.1 配置加载优先级

```
环境变量 > TOML 配置文件 > 硬编码默认值
```

### 7.2 默认配置

```toml
# 服务器配置
[server]
bind_host = "0.0.0.0"
bind_port = 3030
log_level = "info"
max_body_size_bytes = 1048576      # 1MB
request_timeout_secs = 30

# WAL 配置
[wal]
data_dir = "data"
rotation_max_entries = 10000       # 每 10000 条轮转

# WebSocket 配置
[websocket]
orderbook_snapshot_interval_ms = 100
max_connections = 1000

# 风控配置
[risk]
automation_enabled = true
liquidation_interval_secs = 5
funding_interval_secs = 3600       # 1 小时
liquidation_auction_window_secs = 30
maintenance_margin_bps = 50        # 0.5%
liquidation_penalty_bps = 100      # 1%
position_cost_resync_interval_ms = 60000
```

---

## 八、真实测试数据

### 8.1 集成测试用例

```rust
// 运行命令: cargo test --package api -- integration
```

| 测试名称 | 验证内容 | 预期结果 |
|----------|----------|----------|
| `integration_full_order_lifecycle` | 完整下单→撮合→成交→持仓更新链路 | 买卖单完全成交，持仓正确更新 |
| `integration_partial_fill_leaves_resting_order` | 部分成交场景 | 部分成交后剩余挂单仍在订单簿 |
| `integration_self_trade_prevention` | 自成交防护 | 同一用户的买卖单不会相互成交 |
| `integration_kill_switch_blocks_orders` | 紧急停机 | Kill Switch 激活后拒绝新订单 |

### 8.2 单元测试覆盖

| Crate | 测试文件 | 测试数量 | 覆盖范围 |
|-------|----------|----------|----------|
| `types` | N/A | 内联测试 | 序列化/反序列化、枚举变体 |
| `instruments` | N/A | 内联测试 | 注册表 CRUD、状态转换 |
| `ledger` | `main_test.rs` (根目录兼容层) | 余额操作、转账 |
| `matching` | `main_test.rs` (根目录兼容层) | 撮合逻辑、订单簿 |
| `risk` | `main_test.rs` (根目录兼容层) | 风控检查、保证金计算 |

### 8.3 典型测试数据示例

#### 下单请求

```json
{
  "market_id": "BTC-USDT-PERP",
  "side": "Buy",
  "order_type": "Limit",
  "time_in_force": "Gtc",
  "amount": 100,
  "price": 5000000,
  "leverage": 10,
  "request_id": "req-001",
  "client_order_id": "client-order-001"
}
```

#### 成交回报

```json
{
  "fill_id": "fill-abc123",
  "order_id": "order-xyz789",
  "market_id": "BTC-USDT-PERP",
  "side": "Buy",
  "price": 5000000,
  "amount": 100,
  "fee": 5000,
  "timestamp": "2026-04-06T10:30:00Z",
  "maker_taker": "Taker"
}
```

#### 账本变更记录 (LedgerDelta)

```json
{
  "seq_no": 42,
  "user_id": "user-demo-001",
  "delta_type": "TradeSettlement",
  "asset": "USDC",
  "amount_change": -5005000,
  "balance_after": 94995000,
  "timestamp": "2026-04-06T10:30:00.123Z"
}
```

#### 排序器记录 (SequencedCommandRecord)

```json
{
  "seq_no": 42,
  "command_type": "NewOrder",
  "user_id": "user-demo-001",
  "market_id": "BTC-USDT-PERP",
  "timestamp": "2026-04-06T10:30:00.100Z"
}
```

---

## 九、崩溃恢复机制

### 9.1 恢复流程

```
系统重启
    │
    ▼
┌──────────────────────────────────┐
│ 1. 加载最新 Matching Snapshot    │
│    → 重建订单簿状态              │
└──────────────────────────────────┘
    │
    ▼
┌──────────────────────────────────┐
│ 2. 重放 Sequencer WAL            │
│    → 恢复序列号计数器            │
│    → 重放快照后的指令            │
└──────────────────────────────────┘
    │
    ▼
┌──────────────────────────────────┐
│ 3. 重放 Ledger WAL               │
│    → 恢复用户余额                │
│    → 验证全局不变量              │
└──────────────────────────────────┘
    │
    ▼
┌──────────────────────────────────┐
│ 4. 同步 Position Cost Ledger     │
│    → 从 Trade Journal 重建       │
└──────────────────────────────────┘
    │
    ▼
┌──────────────────────────────────┐
│ 5. 自检通过 → 接受新请求         │
└──────────────────────────────────┘
```

### 9.2 数据一致性保证

| 机制 | 保证级别 | 说明 |
|------|----------|------|
| WAL Append-Only | 崩溃安全 | 仅追加写入，崩溃不丢数据 |
| Group Commit | 性能优化 | 批量 fsync，减少 IO 次数 |
| Snapshot + Replay | 快速恢复 | 快照缩短重放路径 |
| Invariant Check | 一致性验证 | 启动时验证资产守恒 |
| Replay Guard | 防重入 | 防止指令重复执行 |

---

## 十、Go 兼容层

### 10.1 保留的 Go 模块

| 模块 | 路径 | 状态 | 说明 |
|------|------|------|------|
| `api/` | `d:\pre_trading\api\` | ⚠️ 兼容层 | 简单 HTTP 代理，已被 rust-exchange 替代 |
| `matching/` | `d:\pre_trading\matching\` | ⚠️ 兼容层 | 基础撮合逻辑，已被 rust-exchange 替代 |
| `ledger/` | `d:\pre_trading\ledger\` | ⚠️ 兼容层 | 账本逻辑，已被 rust-exchange 替代 |
| `risk/` | `d:\pre_trading\risk\` | ⚠️ 兼容层 | 风控逻辑，已被 rust-exchange 替代 |
| `indexer/` | `d:\pre_trading\indexer\` | ⚠️ 兼容层 | 数据索引，已被 rust-exchange 替代 |
| `simulator/` | `d:\pre_trading\simulator\` | ⚠️ 兼容层 | 模拟器，功能待迁移 |

> **注意**：这些 Go 模块是早期原型，当前生产路径为 `rust-exchange`。它们保留用于参考和渐进式迁移。

### 10.2 启动脚本

```powershell
# 主启动脚本（推荐）
.\start.ps1

# 系统验证脚本
.\verify_system.ps1
```

---

## 十一、性能特征

### 11.1 设计目标

| 指标 | 目标值 | 实现方式 |
|------|--------|----------|
| 订单延迟 | < 1ms P99 | 内存撮合 + 异步 WAL |
| 吞吐量 | > 100K TPS | 分区并行撮合 |
| 恢复时间 | < 5s | 快照 + 增量重放 |
| 连接数 | > 1000 WS | 异步 WebSocket Hub |

### 11.2 并发模型

```
┌─────────────────────────────────────────────┐
│                    Tokio Runtime             │
│                                             │
│  ┌───────────┐  ┌───────────┐  ┌─────────┐ │
│  │ HTTP Pool │  │ WS Hub    │  │ Workers │ │
│  │ (warp)    │  │ (broadcast)│  │ (auto)  │ │
│  └─────┬─────┘  └─────┬─────┘  └────┬────┘ │
│        │              │              │       │
│  ┌─────▼──────────────▼──────────────▼────┐ │
│  │          Shared State (Arc)            │ │
│  │  Ledger · Sequencer · Risk · Matching  │ │
│  │  (DashMap + parking_lot::Mutex)        │ │
│  └────────────────────────────────────────┘ │
└─────────────────────────────────────────────┘
```

---

## 十二、运维指南

### 12.1 常用命令

```bash
# 构建 Release
cd rust-exchange && cargo build --release

# 运行测试
cargo test --workspace

# 运行特定 crate 测试
cargo test --package matching
cargo test --package risk
cargo test --package ledger

# 运行集成测试
cargo test --package api -- integration

# 启动服务
cargo run --package api --release

# 清理构建产物
cargo clean
```

### 12.2 日志格式

```json
{
  "timestamp": "2026-04-06T10:30:00.123Z",
  "level": "INFO",
  "target": "api::trading",
  "message": "Order submitted",
  "fields": {
    "user_id": "user-demo-001",
    "market_id": "BTC-USDT-PERP",
    "seq_no": 42
  }
}
```

### 12.3 Prometheus 指标

| 指标名 | 类型 | 说明 |
|--------|------|------|
| `exchange_orders_total` | Counter | 总订单数 |
| `exchange_fills_total` | Counter | 总成交数 |
| `exchange_latency_seconds` | Histogram | 订单处理延迟 |
| `websocket_connections` | Gauge | 当前 WS 连接数 |
| `wal_entries_total` | Counter | WAL 写入总数 |

---

## 十三、数据文件清单

### 运行时生成的数据文件

```
data/
├── ledger.wal.jsonl              # 账本变更流水
├── sequencer.wal.jsonl           # 排序指令流水
├── matching.snapshot.jsonl       # 撮合快照
├── trade_journal.wal.jsonl       # 成交记录
├── trade_settlement.wal.jsonl    # 结算记录
├── instruments.registry.jsonl    # 合约注册表
├── funding_rates.jsonl           # 资金费率
├── liquidation.queue.jsonl       # 清算队列
├── liquidation.auction.jsonl     # 清算拍卖
├── adl.governance.jsonl          # ADL 治理
├── liquidation.policy.jsonl      # 清算策略
├── index.price.jsonl             # 指数价格
├── index.source.policy.jsonl     # 指数源策略
├── position.cost.state.jsonl     # 持仓成本状态
├── position.cost.events.jsonl    # 持仓成本事件
├── governance.actions.jsonl      # 治理操作
├── risk_automation.audit.jsonl   # 风控审计
└── replay_guard.jsonl            # 重放保护
```

---

## 十四、架构决策记录

| 决策 | 选择 | 理由 |
|------|------|------|
| 语言 | Rust | 零成本抽象、内存安全、高并发 |
| HTTP 框架 | warp | 组合式过滤器、原生 async |
| 持久化 | JSONL WAL | 简单可靠、易调试、崩溃安全 |
| 撮合架构 | 分区引擎 | 水平扩展、市场隔离 |
| 状态管理 | 内存 + WAL | 低延迟读取、WAL 保障持久化 |
| 事件分发 | EventBus | 解耦组件、支持多消费者 |

---

## 十五、已知限制

1. **单进程架构**：当前为单进程设计，不支持多节点水平扩展
2. **内存状态**：所有状态驻留内存，受限于单机 RAM
3. **JSONL WAL**：相比二进制格式有序列化开销
4. **Go 兼容层**：Go 模块仅为历史遗留，不作为生产路径
5. **外部依赖**：指数价格、资金费率等需要外部数据源
