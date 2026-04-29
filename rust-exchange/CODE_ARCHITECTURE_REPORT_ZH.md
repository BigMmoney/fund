# Rust 交易系统 — 完整架构与逻辑报告

**日期:** 2026-04-07  
**版本:** v0.1.0  
**语言:** Rust 2021 Edition  
**审计状态:** 两轮深度安全审计完成，513/513 测试全部通过

---

## 一、系统概览

本系统是一个**高性能、分片式撮合引擎**（Partitioned Matching Engine），采用 Rust 编写，设计目标为微秒级延迟、水平可扩展的交易核心。系统由 **10 个 Crate** 组成，遵循严格的分层架构：

```
┌─────────────────────────────────────────────────────────────┐
│                      API Layer (warp)                       │
│  crates/api — REST/WebSocket 接口、认证、路由、DTO 转换       │
├─────────────────────────────────────────────────────────────┤
│                    Core Trading Engine                      │
│                                                             │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌────────────┐  │
│  │sequencer │→│  risk    │→│ matching │→│  ledger    │  │
│  │排序去重  │  │风控检查  │  │撮合引擎   │  │账务记账     │  │
│  └──────────┘  └──────────┘  └──────────┘  └────────────┘  │
│                                                             │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐                  │
│  │eventbus  │  │projections│ │persistence│                  │
│  │事件总线  │  │投影计算   │ │持久化/WAL  │                  │
│  └──────────┘  └──────────┘  └──────────┘                  │
├─────────────────────────────────────────────────────────────┤
│                   Foundation Layer                          │
│                                                             │
│  ┌──────────┐  ┌──────────┐                                 │
│  │ types    │  │instruments│                                 │
│  │类型定义  │  │品种注册   │                                 │
│  └──────────┘  └──────────┘                                 │
└─────────────────────────────────────────────────────────────┘
```

### Crate 职责矩阵

| Crate | 行数 | 核心职责 | 关键数据结构 |
|-------|------|----------|-------------|
| `types` | ~1800 | 全系统共享类型定义 | `Side`, `OrderType`, `TimeInForce`, `Command`, `InstrumentSpec`, `FeeSchedule` |
| `instruments` | ~300 | 品种注册与策略解析 | `InstrumentRegistry`, `InstrumentStrategy` |
| `sequencer` | ~600 | 命令排序、去重、WAL 写入 | `Sequencer`, `SequencedCommandRecord` |
| `risk` | ~1200 | 风险检查（保证金、持仓限额） | `RiskEngine`, `PositionTracker` |
| `matching` | ~6000+ | 分区撮合引擎（核心） | `PartitionedMatchingEngine`, `MarketRuntime`, `RestingOrder` |
| `ledger` | ~500 | 复式记账、余额变更 | `DoubleEntryLedger`, `LedgerEntry`, `Account` |
| `eventbus` | ~200 | 内存事件发布/订阅 | `EventBus`, `EventSubscriber` |
| `projections` | ~800 | 从事件流计算派生状态 | `ProjectionEngine`, `UserBalanceProjection` |
| `persistence` | ~400 | WAL/快照存储抽象 | `WalStore`, `SnapshotStore`, `InMemoryWal` |
| `api` | ~5000+ | HTTP/WebSocket 服务层 | Warp 路由、DTO、认证中间件 |

---

## 二、核心数据流：一笔订单的完整生命周期

### 2.1 请求入口（API Layer）

```
Client POST /api/v1/orders
    ↓
with_principal()          ← HMAC-SHA256 认证，提取 AuthenticatedPrincipal
    ↓
body_limit(64KB)          ← 请求体大小限制
    ↓
warp::body::json<OrderRequest>()  ← JSON 反序列化
    ↓
validate_order()          ← 字段校验（价格、数量、品种存在性）
    ↓
build_new_order_command() ← 转换为 NewOrderCommand
```

### 2.2 排序与去重（Sequencer）

```rust
// crates/sequencer/src/lib.rs
pub fn sequence(&self, command: Command) -> Result<Command, SequencerError> {
    // 1. 使用 DashMap Entry API 原子化检查 request_id 是否已存在
    // 2. 仅当是新请求时，分配全局递增序列号 (next_seq.fetch_add)
    // 3. 标记生命周期为 Sequenced
    // 4. 插入 DashMap，返回成功
    // 重复请求直接返回 DuplicateRequest 错误，不消耗序列号
}
```

**关键设计决策：** 使用 DashMap 的分片锁而非全局 Mutex，实现高并发下的无竞争去重。每个分片的锁粒度极小，保证纳秒级的重复检测。

### 2.3 风险控制（Risk Engine）

```rust
// crates/risk/src/lib.rs
pub fn check_order(&self, command: &NewOrderCommand, instrument: &InstrumentSpec) -> Result<(), RiskError> {
    // 1. 品种状态检查（是否暂停交易）
    // 2. 价格合理性检查（涨跌停板）
    // 3. 数量合理性检查（最小/最大下单量）
    // 4. 保证金充足性检查（衍生品）
    // 5. 持仓限额检查
    // 6. 自成交预防（STP）组检查
}
```

### 2.4 撮合执行（Matching Engine — 系统核心）

这是整个系统最复杂的部分，占代码量的 60%+。

#### 分区架构

```
PartitionedMatchingEngine
├── Partition 0 (市场: perp:btc-usdt)
│   ├── bids: BTreeMap<i64, VecDeque<String>>  ← 买盘价格队列
│   ├── asks: BTreeMap<i64, VecDeque<String>>  ← 卖盘价格队列
│   ├── orders: HashMap<String, RestingOrder>  ← 挂单索引
│   ├── user_orders: HashMap<String, Vec<String>> ← 用户挂单索引
│   ├── trigger_orders: HashMap<String, TriggerOrder> ← 条件单
│   ├── mm_fill_trackers: HashMap<String, MmFillTracker> ← 做市商追踪
│   ├── user_volume_30d: HashMap<String, i64>  ← 用户30天成交量
│   └── recent_events: VecDeque<RecentMarketEvent> ← 最近市场事件
├── Partition 1 (市场: perp:eth-usdt)
│   └── ...
└── Partition N
    └── ...
```

**分区原则：** 每个市场独立一个 Partition，不同市场之间完全隔离。同一 Partition 内的操作是串行的（通过 `Command` 通道），保证了单市场内的强一致性。

#### 撮合算法

```rust
fn match_order(market: &mut MarketRuntime, command: NewOrderCommand, instrument: &InstrumentSpec) -> MatchResult {
    // 1. 确定对手盘方向（买单扫卖盘，卖单扫买盘）
    // 2. 价格优先、时间优先（BTreeMap 天然有序，VecDeque FIFO）
    // 3. 冰山订单处理：display_qty 控制可见数量
    // 4. STP 自成交预防：跳过同组用户的挂单
    // 5. 做市商保护：检查 fill_window 内的累计成交量
    // 6. 触发条件单：价格触及 trigger_price 时激活
    // 7. 生成成交记录（TradeRecord）和订单状态变更
    // 8. 更新 TradeStats（VWAP、最高最低成交价等）
    // 9. 更新 user_volume_30d 和 mm_fill_trackers
}
```

#### 订单簿数据结构

```
BTreeMap<i64, VecDeque<String>>
  ↑              ↑
  价格           该价格上的订单ID队列（FIFO）
  
优势：
- BTreeMap 保证价格有序遍历（O(log N) 查找，O(K) 遍历 K 个价位）
- VecDeque 保证同价位时间优先（O(1) 入队/出队）
- 冰山订单通过 visible_qty 控制队列头部订单的可见部分
```

### 2.5 账务处理（Ledger）

```rust
// crates/ledger/src/lib.rs
pub fn commit(&self, op: LedgerOp) -> Result<(), Error> {
    // 1. 幂等性检查：op_id 是否已消费
    // 2. 预验证：借贷平衡（sum_debits == sum_credits）
    // 3. 余额检查：不允许账户透支（除非允许负余额）
    // 4. 应用变更：借记扣款，贷记入账
    // 5. 版本号递增：用于乐观并发控制
    // 6. 发布余额变更事件
}
```

**关键修复：** 所有算术运算已从不安全的 `+=`/`-=` 改为 `checked_add`/`saturating_sub`，防止 release 模式下的溢出行为。

### 2.6 事件分发与投影

```
撮合结果 → EventBus → 多个 Subscriber
                    ├── Projections（用户持仓、余额、盈亏计算）
                    ├── Persistence（WAL 追加写入）
                    └── API WebSocket（实时推送给前端）
```

---

## 三、关键架构决策与设计模式

### 3.1 不可变命令流（Immutable Command Flow）

所有交易指令以 `Command` 枚举形式流经系统：

```rust
pub enum Command {
    NewOrder(NewOrderCommand),
    CancelOrder(CancelOrderCommand),
    MassCancelByUser(MassCancelByUserCommand),
    MassCancelByMarket(MassCancelByMarketCommand),
    ModifyOrder(ModifyOrderCommand),
    // ...
}
```

**优势：** 单一数据流使得排序、日志、回放、重放都变得简单且可验证。

### 3.2 快照与恢复（Snapshot & Recovery）

```
MarketSnapshot
├── RestingOrderSnapshot[]  ← 所有挂单的序列化
├── TriggerOrderSnapshot[]  ← 所有条件单的序列化
├── TradeStats              ← 统计信息
├── ReferencePriceSource[]  ← 参考价格源
└── last_trade_price        ← 最后成交价

恢复流程：
1. 加载最新快照 → 重建订单簿
2. 重放 WAL 中快照之后的条目 → 追平到最新状态
3. 验证状态一致性
```

**关键修复：** 快照现在正确保存 `stp_group_id`、`is_market_maker`、`expires_at` 等字段，确保恢复后 STP 和做市商保护功能正常。

### 3.3 分区并发模型

```
主线程/管理线程
    ├── 创建 N 个 Partition Handle（每个市场一个）
    ├── 通过 Channel 发送 Command 到对应 Partition
    └── 接收 Response 返回给调用方

每个 Partition 内部：
    ├── 单线程处理（避免内部竞争）
    ├── 通过 mpsc channel 接收命令
    └── 通过 oneshot channel 返回结果
```

**性能特征：** 单分区内串行保证一致性，分区间并行保证吞吐量。N 个市场 = N 倍吞吐。

### 3.4 做市商保护机制

```rust
struct MmFillTracker {
    fills: VecDeque<(Instant, i64, i64)>,  // (时间戳, 成交量, 成交额)
}

// 检查窗口期内的累计成交量
// 如果超过 max_delta_qty 或 max_notional_window，自动撤单
// 防止做市商在极端行情下被过度成交
```

### 3.5 费用体系

```
基础费率：maker_fee_bps / taker_fee_bps（万分之一单位）
    ↓
阶梯费率（FeeSchedule）：
├── Tier 1: 30天成交量 < 100万 → maker: 5bps, taker: 10bps
├── Tier 2: 30天成交量 < 1000万 → maker: 3bps, taker: 8bps
└── Tier 3: 30天成交量 ≥ 1000万 → maker: 0bps, taker: 6bps
```

---

## 四、本轮安全修复汇总

### 4.1 已修复漏洞（本轮新增）

| 编号 | 严重度 | 问题 | 修复方案 | 文件 |
|------|--------|------|---------|------|
| P0-1 | 严重 | `user_volume_30d` 无限增长导致 OOM | 添加容量上限 (100K) + 定期淘汰 10% 最旧条目 | `matching/partitioned.rs` |
| P0-2 | 严重 | `mm_fill_trackers` 和 `recent_events` 无限增长 | 添加 `cap_fills()` 方法 + 定期淘汰 + 计数器上限 | `matching/partitioned.rs` |
| P2-1 | 中等 | Ledger 借贷求和使用未检查加法 | 改用 `checked_add`，溢出时返回错误 | `ledger/lib.rs` |
| P2-2 | 中等 | Ledger 余额变更使用未检查加减法 | 改用 `saturating_add/sub`，防止溢出回绕 | `ledger/lib.rs` |
| P2-5 | 中等 | 快照恢复丢失 `stp_group_id` | 将字段加入 `RestingOrderSnapshot` 和 `TriggerOrderSnapshot` | `matching/partitioned.rs` |
| P2-6 | 中等 | `can_fully_fill` 中使用未检查减法 | 改用 `saturating_sub` | `matching/partitioned.rs` |
| SEQ-1 | 高 | Sequencer TOCTOU 竞态条件 | 将 `fetch_add` 移入 Entry API 的 Vacant 分支，消除竞争窗口 | `sequencer/lib.rs` |

### 4.2 前轮已修复漏洞（回顾）

| 编号 | 严重度 | 问题 | 修复方案 |
|------|--------|------|---------|
| P0-1 | 严重 | OpenAPI/Swagger 端点无认证保护 | 添加 `with_principal()` + admin 角色检查 |
| P0-4 | 严重 | 充值端点泄露内部错误信息 | 添加 `sanitize_internal_error()` 过滤 |
| P1-1 | 高 | 仓位充值无金额上限 | 添加 `MAX_SINGLE_DEPOSIT` (10B subunits) |
| P1-2 | 高 | 提现端点缺少请求体限制 | 添加 `body_limit()` 过滤器 |
| P1-5 | 高 | 批量订单数组无大小限制 | 自定义反序列化器限制 `MAX_BATCH_SIZE = 100` |
| P2-3 | 中 | `IdempotencyCache` 非原子操作 | 改用 DashMap Entry API 原子化 |
| P2-5 | 中 | 充值请求 ID 无长度校验 | 添加 1-256 字符长度限制 |

### 4.3 编译与测试验证

```
cargo check --release    → 零错误
cargo test --release     → 513/513 通过，0 失败

分布：
  types:        217 passed
  eventbus:       6 passed
  instruments:   13 passed
  ledger:        16 passed
  matching:     105 passed
  persistence:   14 passed
  projections:   20 passed
  risk:          68 passed
  sequencer:      9 passed
  api:           45 passed
```

---

## 五、尚未修复的风险项

### 5.1 需要基础设施变更（延期）

| 编号 | 严重度 | 问题 | 所需变更 |
|------|--------|------|---------|
| P0-2 (旧) | 严重 | HMAC 密钥存储在环境变量 | 迁移到 Vault/文件密钥管理 |
| P0-3 (旧) | 严重 | 服务端信任客户端提供的 role 声明 | 服务端独立查询角色数据库 |

### 5.2 有缓解措施但非完美（监控中）

| 编号 | 严重度 | 问题 | 当前缓解措施 |
|------|--------|------|-------------|
| P2-1 (旧) | 中 | LiquidationQueueStore 迭代+修改竞态 | 单线程处理降低概率 |
| P2-2 (旧) | 中 | WAL 轮转期间的并发读取安全 | 文件级锁 |
| P2-4 (旧) | 低 | 充值端点错误时仍返回 HTTP 200 | 业务层错误码区分 |

---

## 六、性能特征

### 6.1 延迟预算（估算）

| 阶段 | 延迟 | 说明 |
|------|------|------|
| API 反序列化 | ~5μs | JSON → DTO 转换 |
| 认证验证 | ~10μs | HMAC-SHA256 计算 |
| Sequencer 排序 | ~1μs | DashMap 分片锁 + fetch_add |
| Risk 检查 | ~5-20μs | 取决于持仓复杂度 |
| Matching 撮合 | ~1-50μs | 取决于扫单深度 |
| Ledger 记账 | ~2μs | 内存哈希表操作 |
| **端到端 (P99)** | **~50-100μs** | 空载情况 |

### 6.2 扩展性

- **水平扩展：** 增加 Partition 数量 = 增加支持的市场数
- **垂直扩展：** 单 Partition 吞吐量受限于单线程处理能力
- **内存使用：** 每 10 万活跃挂单约占用 50-100MB RAM
- **瓶颈预期：** 匹配引擎的单线程处理是天然瓶颈，无法通过增加 CPU 核数提升单市场吞吐

---

## 七、技术栈总结

| 组件 | 技术选型 | 版本 |
|------|---------|------|
| Web 框架 | Warp | 0.3.7 |
| 并发哈希表 | DashMap | 6.1 |
| 互斥锁 | parking_lot | 0.12 |
| 序列化 | serde + serde_json | 1.0 |
| 时间处理 | chrono | 0.4 |
| 错误处理 | thiserror + anyhow | 2.0 + 1.0 |
| 日志 | tracing + tracing-subscriber | 0.1 + 0.3 |
| 唯一 ID | uuid (v4) | 1.11 |
| 异步运行时 | tokio | 1.42 |
| 编译器 | Rust | 1.88.0 (stable) |
| 目标平台 | x86_64-pc-windows-gnu | — |

---

## 八、代码质量指标

| 指标 | 数值 |
|------|------|
| 总 Rust 源文件数 | ~56 个 |
| 总代码行数 | ~17,000+ 行 |
| 单元测试覆盖 | 513 个测试用例 |
| Crate 数量 | 10 个 |
| 安全漏洞（已修复） | 14 个 |
| 安全漏洞（待修复） | 4 个（需基础设施变更） |
| 编译警告 | 0（release 模式） |

---

*报告生成于 2026-04-07，基于 commit d82a444 及后续安全修复提交。*
