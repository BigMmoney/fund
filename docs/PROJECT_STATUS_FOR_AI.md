# Rust Exchange 当前技术状态与开发交接文档

> 建议文件名：`PROJECT_STATUS_FOR_AI.md`  
> 用途：放在 VSCode 工作区根目录，方便其他 AI、开发者或新协作者快速理解当前项目状态、架构边界、已完成内容和后续优先级。  
> 当前判断基于仓库 README / 中文后端 README 中描述的 Rust Exchange 当前状态。

---

## 1. 项目一句话定位

`rust-exchange` 是当前仓库里的 **正式交易核心**。

项目已经不再是 Go / Rust 双核心状态：

- Rust 负责真实订单状态
- Rust 负责真实成交状态
- Rust 负责余额和持仓状态
- Rust 负责恢复逻辑
- Rust 负责风控执行路径
- Go API 如仍存在，只应视为兼容层或迁移层，不应作为交易事实源

当前系统可以理解为：

> 一个 Rust 编写的交易所核心 v1，已经打通认证、定序、风控、撮合、账本、成交日志、WAL 恢复和基础风险自动化；适合进入受控 beta / staging / 小额灰度部署，但还不应直接作为完整公开生产交易所无限制开放。

---

## 2. 当前总体评分

| 维度 | 评分 | 状态判断 |
|---|---:|---|
| 架构完整度 | 8 / 10 | 主链路清晰，crate 职责拆分合理 |
| 核心正确性 | 7.5 / 10 | 已修复多项高风险一致性问题，但仍需更长时间实战验证 |
| 性能 | 7 / 10 | 基准性能可用，写入路径约 30ms 级别；高并发下已有锁竞争迹象 |
| 安全基础 | 7 / 10 | HMAC、RBAC、限流、双签、WAL 校验等基础已具备 |
| 部署成熟度 | 7 / 10 | Docker / K8s / health / ready / WAL volume / Grafana 基础已具备 |
| 产品完整度 | 5.5 / 10 | Spot / Margin / Perp 有核心语义，期权、交割合约、OTC、理财未完成 |
| 强平与资金费率 | 5 / 10 | 有 v1 自动化，但缺完整生产级强平体系和自动 funding rate 生成 |
| 生产运营能力 | 6 / 10 | metrics 和日志已有，但风控看板、审计查询、灾备演练仍需加强 |

综合判断：

- **作为工程 v1：约 7.2 / 10**
- **作为公开真实资金生产交易所：约 5.8–6.3 / 10**

---

## 3. 是否可以部署第一版本

### 结论

可以部署第一版本，但必须限定为：

- dev
- staging
- paper trading
- closed beta
- 小额限额实盘
- 受控市场灰度

不建议直接作为：

- 公开生产交易所
- 无限资金规模实盘
- 高杠杆永续交易平台
- 大规模真实用户撮合系统

### 推荐第一版定位

第一版应定位为：

> Rust Exchange v1 beta：受控交易核心验证版。

建议限制：

- 用户白名单
- 市场数量限制
- 单用户资金上限
- 单笔订单数量上限
- 杠杆上限，例如 2x–5x
- 初期只开放 Spot，Perp / Margin 小范围测试
- 自动强平谨慎开启
- 所有 admin 操作必须审计
- 必须保留 kill switch / market halt 能力

---

## 4. 当前正式架构主链路

当前官方写路径：

```text
HTTP Gateway
-> Auth / Principal
-> Sequencer WAL
-> Risk Reserve / Check
-> Partitioned Matching
-> Ledger Commit
-> Trade Journal
-> Snapshot + WAL Replay
-> Read Models / Risk Automation
```

展开理解：

1. API 接收请求
2. 从认证上下文中提取 principal
3. 区分 user / admin action
4. sequencer 追加 command WAL 并分配 command_seq
5. risk 执行预检查和资金 / 仓位 reserve
6. matching 在分区内执行订单簿状态机
7. ledger 提交财务状态变化
8. trade journal 记录成交事实
9. snapshot 持久化撮合状态
10. 重启时通过 snapshot + partition-aware sequencer replay 恢复

---

## 5. Workspace / Crate 职责

| Crate | 当前职责 |
|---|---|
| `crates/types` | 领域类型：订单、账户、命令、品种规格、角色、生命周期状态 |
| `crates/eventbus` | 进程内事件分发，基于 Tokio broadcast |
| `crates/instruments` | 品种注册表，支持持久化 instrument registry |
| `crates/sequencer` | 命令定序，维护单调 command_seq、去重、WAL 恢复 |
| `crates/persistence` | WAL 抽象和 JSONL 文件持久化 |
| `crates/ledger` | 复式账本、余额、hold、spot / derivative position、op_id 幂等 |
| `crates/risk` | 风控 reserve / release、margin snapshot、强平评估、资金费率结算 |
| `crates/matching` | 分区撮合、订单簿、价格时间优先、自成交防护、replace、snapshot / restore |
| `crates/projections` | 仓位、margin、PnL 读模型，目前是 pull-based，不是独立服务 |
| `crates/api` | HTTP / WebSocket 网关，路由组合、认证、限流、恢复启动、风控自动化 scheduler |

---

## 6. API 模块当前拆分

重点模块：

| 模块 | 职责 |
|---|---|
| `crates/api/src/trading.rs` | 用户交易写路径：intent、submit、cancel、replace、批量撤单 |
| `crates/api/src/control.rs` | 管理员交易控制：deposit、市场批量撤单、kill switch、market state、reference price |
| `crates/api/src/accounts.rs` | balances、positions、margin、pnl、orders、deposits |
| `crates/api/src/markets.rs` | markets、market detail、order book、trades、history、stats、matching status |
| `crates/api/src/admin.rs` | instruments、funding-rate 控制、risk events、funding settlement |
| `crates/api/src/pricing.rs` | index source store、arbitration、fair-price routes |
| `crates/api/src/governance.rs` | pending governance actions、dual approval workflow |
| `crates/api/src/liquidation.rs` | liquidation queue override、worker、auction、insurance routes |
| `crates/api/src/security.rs` | internal auth、principal filters、role / subject guard |
| `crates/api/src/helpers.rs` | request-id normalization、audit helpers、lifecycle marker helpers |
| `crates/api/src/stores.rs` | persistent store builders、registry seed wiring |
| `crates/api/src/bootstrap.rs` | runtime bootstrap、WAL recovery、partition-aware replay、automation startup |
| `crates/api/src/main.rs` | 顶层路由组合、CORS、静态资源、HTTP server 入口 |

---

## 7. 当前已实现产品范围

### 已经具备真实核心语义

#### Spot

- 买方现金 reservation
- 卖方 spot inventory reservation
- 通过 ledger 结算

#### Margin

- 杠杆相关订单字段
- 衍生品式 position path
- margin snapshot
- liquidation evaluation

#### Perpetual

- derivative position accounting
- funding preview
- funding settlement
- manual liquidation path
- automated liquidation path
- margin snapshot support

### 尚未正式完成

以下不要当成已完成产品：

- delivery futures
- options
- OTC negotiation venue
- wealth / structured products

---

## 8. 撮合与交易规则状态

### 已具备

- 分区撮合引擎
- price-time priority
- BTreeMap 订单簿
- limit / market / stop / IOC / FOK / GTD 等订单语义基础
- self-trade prevention
- replace = cancel + new
- invalid replacement 不应破坏原有效挂单
- settlement failure 不应留下半提交 book state
- trade journal failure 不应留下半交易状态
- severe commit-path failure 应 halt market，而不是静默损坏状态

### 当前 self-trade prevention 默认策略

```text
reject taker
```

即拒绝新来的 taker-side action，而不是隐式修改 resting side。

### replace 语义

replace 明确建模为：

```text
cancel + new
```

含义：

- 原订单优先级丢失
- outward behavior 需要保持原子性
- invalid replacement 不能删除原有效订单

---

## 9. 恢复模型

当前恢复不是 snapshot-only。

当前恢复模型：

```text
snapshot restore + sequencer WAL replay(after per-partition snapshot boundary)
```

重要性质：

- replay 按 partition 判断
- 每个 partition 维护自己的 `last_applied_command_seq`
- 一个快分区的 snapshot 不能导致慢分区跳过有效 command

这是当前系统正确性里非常关键的一点。

---

## 10. 数据持久化状态

运行时数据主要在 `data/` 目录下，以 JSONL WAL 方式持久化。

典型文件：

| 文件 | 用途 |
|---|---|
| `data/ledger.wal.jsonl` | 复式账本变动 |
| `data/sequencer.wal.jsonl` | 定序命令记录 |
| `data/matching.snapshot.jsonl` | 订单簿快照 |
| `data/trade_journal.wal.jsonl` | 成交日志 |
| `data/trade_settlement.wal.jsonl` | 结算记录 |
| `data/instruments.registry.jsonl` | 品种定义 |
| `data/funding_rates.jsonl` | 资金费率 |
| `data/position.cost.state.jsonl` | 持仓成本基准 |
| `data/liquidation.queue.jsonl` | 待强平队列 |
| `data/adl.governance.jsonl` | ADL 候选排名 |
| `data/index.price.jsonl` | 外部指数价格 |
| `data/governance.actions.jsonl` | 治理操作 |
| `data/withdrawals.wal.jsonl` | 提现请求 |
| `data/address_whitelist.wal.jsonl` | 提现地址白名单 |
| `data/transfers.wal.jsonl` | 内部转账 |
| `data/stop_orders.wal.jsonl` | 止损止盈单 |

WAL 默认轮转策略：

```text
rotation_max_entries = 100000
```

---

## 11. 当前配置状态

主配置文件：

```text
config/exchange.toml
```

可通过环境变量覆盖：

```text
EXCHANGE_CONFIG_PATH
```

配置优先级：

```text
环境变量 > config/exchange.toml > 硬编码默认值
```

关键配置区域：

```toml
[server]
bind_host = "127.0.0.1"
bind_port = 3030
log_level = "info"

[wal]
data_dir = "data"
rotation_max_entries = 100000
group_commit_size = 64

[risk]
automation_enabled = false
liquidation_interval_secs = 30
funding_interval_secs = 60

[websocket]
orderbook_snapshot_interval_ms = 200
max_connections = 1024
```

敏感配置：

```text
INTERNAL_AUTH_SHARED_SECRET
INTERNAL_AUTH_SHARED_SECRET_FILE
SERVER_ROLE_MAPPING_FILE
```

生产环境优先使用：

```text
INTERNAL_AUTH_SHARED_SECRET_FILE
SERVER_ROLE_MAPPING_FILE
```

不要在生产环境使用开发明文 secret。

---

## 12. 安全状态

### 已具备

- HMAC-SHA256 内部签名认证
- Unix 毫秒时间戳，默认 30 秒窗口防重放
- body sha256 校验
- nonce 防重放
- RBAC：user / admin / system
- IP / user / admin 多级限流
- request body size limit
- structured rejection handling
- admin-only control plane
- dual approval governance
- WAL CRC-32 校验
- 默认绑定 loopback：`127.0.0.1`
- Kubernetes / Compose 支持非 root、只读根文件系统、drop capabilities 等硬化配置

### 仍需加强

- TLS / ingress / gateway 实际部署校验
- secret rotation
- key versioning
- 更完整审计查询界面
- 风控操作告警
- admin 操作强制双签覆盖范围确认
- 外部安全审计
- 压测下认证和限流路径稳定性

---

## 13. 观测与运维状态

### 已有 endpoints

```text
GET /health
GET /ready
GET /metrics
GET /metrics/prometheus
GET /version
GET /openapi.json
```

### 已有 metrics 类型

- orders received / filled / rejected / cancelled
- settlements committed
- wal appends / wal errors
- websocket active connections
- http requests / http errors
- match latency
- wal append latency
- risk check latency
- matching core latency
- settlement latency
- partition fills

### Grafana

已有 dashboard 文件：

```text
deploy/grafana/exchange-dashboard.json
```

### 仍需补充

- error budget
- SLO / SLA 定义
- pager / alert rules
- runbook
- market halt 后恢复流程
- WAL restore 演练记录
- 24h / 72h soak 报告
- 数据一致性巡检任务

---

## 14. 性能状态

README 中记录的最新摘要大致为：

| 场景 | 状态 |
|---|---|
| read endpoint | 约 3–5ms |
| sequential order | P50 约 31ms |
| full pipeline | P50 约 29ms，P99 约 53ms |
| 3min soak | 1882 样本，0% 失败 |
| 30min soak | 36,040 笔订单，0 失败，P99 约 42ms |

判断：

- 当前性能足够支撑 v1 beta
- 写路径延迟符合 WAL / risk / ledger 持久化预期
- 10 worker 并发已出现锁竞争迹象
- 后续要重点分析 matching / ledger / risk hot path 的锁和分片设计

---

## 15. 已修复的重要正确性问题

当前文档明确认为这些高风险问题已经修复或关闭：

- non-atomic replace 可能删除有效 resting order
- partition inflight accounting race
- snapshot / replay boundary 导致慢分区 command 被跳过
- settlement failure 留下 half-applied book state
- trade journal failure 留下 partial trade state
- in-memory-only instrument registry 缺少持久化事实源
- risk layer 只有 evaluation，没有基础 execution flow
- 缺少 formal automation audit stream

这些修复意味着主交易核心已经不是早期不可信状态。

---

## 16. 当前最大缺口

### A. 强平系统不完整

未完成：

- insurance fund
- bankruptcy price
- liquidation waterfall
- liquidation auction
- liquidation order book

当前有 v1 自动强平，但更像基础执行能力，不是完整生产级强平体系。

### B. 资金费率系统不完整

未完成：

- premium / index based automatic rate generation
- stricter funding-clock semantics
- global netting
- batch optimization

当前 funding rate 更偏 operator-managed control plane。

### C. 读侧仍轻量

未完成：

- standalone projection service
- richer risk monitoring views
- separate market-data read store
- audit read store
- 更完整的运营后台查询能力

### D. 产品线未完成

未完成：

- delivery futures
- options
- OTC
- wealth / structured products

### E. 生产验证不足

仍建议完成：

- 24h soak
- 72h soak
- WAL backup restore drill
- abnormal restart drill
- market halt / resume drill
- chaos test
- security review
- admin operation audit test

---

## 17. 建议开发优先级

### P0：部署前必须确认

- `cargo test --workspace` 全部通过
- E2E 下单 / 撮合 / 撤单 / 成交 / 余额 / 持仓通过
- WAL recovery 测试通过
- restart after errors 测试通过
- ledger invariant 检查通过
- kill switch / market halt 实际验证
- Prometheus / Grafana 接入
- WAL 持久化 volume 验证
- WAL backup + restore 实际演练
- 生产 secret file 配置验证

### P1：v1 beta 必须补强

- closed beta 白名单
- 用户资金上限
- 市场级别限额
- 杠杆限制
- admin 操作审计
- 风控错误告警
- order / ledger / trade journal 一致性巡检
- 2–6 小时 soak
- 基础 runbook

### P2：公开实盘前必须补强

- insurance fund
- bankruptcy price
- liquidation waterfall
- liquidation auction
- 自动 funding rate 生成
- 独立 projection / read service
- 风控后台
- 审计查询后台
- 24h+ soak
- 灾备恢复演练
- 外部安全审计
- key rotation
- 多实例 / HA 方案评估

---

## 18. 给其他 AI 的重要开发原则

如果你是在 VSCode 里接手这个项目，请遵守以下原则：

### 不要重写主链路

当前主链路已经闭环，优先补生产子系统，不要轻易推倒：

```text
auth -> sequencer -> risk -> matching -> ledger -> journal -> snapshot/replay
```

### 不要绕过 Rust 核心

所有真实交易状态必须经过 Rust path：

- 不要让 Go API 产生真实订单状态
- 不要让脚本直接改余额
- 不要绕过 sequencer
- 不要绕过 risk reserve
- 不要绕过 ledger commit
- 不要直接改 WAL 除非是专门的 migration / repair tool

### 所有状态修改必须考虑恢复

任何新功能都要回答：

1. 是否写 WAL？
2. 是否有 op_id / request_id 幂等？
3. crash 后如何 replay？
4. snapshot 边界如何处理？
5. 是否会产生半提交状态？
6. 失败时是否 halt market 或 rollback？

### 涉及资金状态必须经过 ledger

不要在业务模块里私自维护“真实余额”。

真实财务状态应以 ledger 为准。

### 涉及交易顺序必须经过 sequencer

不要在 API 层、matching 层随意生成事实顺序。

正式顺序以 `command_seq` 为准。

### 涉及风控必须先 reserve / check

不要让订单直接进入 matching。

正式路径必须先经过 risk。

---

## 19. 推荐第一版本部署方案

### 阶段 1：本地 / dev

目标：

- 编译通过
- 单元测试通过
- quick perf 通过
- 本地 WAL 正常生成
- health / ready 正常

命令示例：

```powershell
cargo build --release
cargo test --workspace
.\scripts\quick_perf_test.ps1
```

### 阶段 2：staging

目标：

- Docker / K8s 部署通过
- Prometheus / Grafana 接入
- WAL volume 挂载
- secret file 加载
- role mapping file 加载
- recovery drill 通过

### 阶段 3：paper trading

目标：

- 连续运行 3–7 天
- 模拟真实用户下单 / 撤单 / 撮合
- 观察 latency、error、WAL size、memory、recovery
- 验证 admin 操作和风控操作

### 阶段 4：closed beta 小额实盘

限制：

- 白名单用户
- 小额资金
- 少数市场
- 低杠杆
- 自动强平保守开启
- admin 人工兜底
- 每日恢复演练 / 对账检查

### 阶段 5：公开实盘

只有在补齐强平、funding、read side、运维、审计、灾备、安全审计后再考虑。

---

## 20. 快速命令清单

### 构建

```bash
cargo build --release
```

### 单元测试

```bash
cargo test --workspace
```

### 运行 API

```bash
./target/release/api
```

Windows:

```powershell
.\target\release\api.exe
```

### 健康检查

```bash
curl http://localhost:3030/health
curl http://localhost:3030/ready
```

### OpenAPI

```bash
curl http://localhost:3030/openapi.json
```

### Metrics

```bash
curl http://localhost:3030/metrics
curl http://localhost:3030/metrics/prometheus
```

### Docker

```bash
docker-compose up -d
```

### Rust crate 测试

```bash
cargo test -p matching
cargo test -p ledger
cargo test -p sequencer
```

### 性能测试

```powershell
.\scripts\quick_perf_test.ps1
.\scripts\benchmark_suite.ps1
.\scripts\soak_test_v2.ps1
.\scripts\cancel_storm_test.ps1
.\scripts\e2e_trading_test.ps1
.\scripts\test_wal_recovery.ps1
.\scripts\test_restart_after_errors.ps1
```

---

## 21. 建议交给 AI 的下一步任务

如果要继续推进项目，可以把下面任务逐个交给 AI：

### 任务 1：部署前检查

> 请检查当前仓库是否满足 v1 beta 部署条件，包括配置、secret、WAL volume、health / ready、Prometheus、Docker / K8s、测试脚本，并输出缺口清单。

### 任务 2：强平体系设计

> 请基于当前 risk / ledger / liquidation 模块，设计 insurance fund、bankruptcy price、liquidation waterfall、auction 的最小生产可用版本。

### 任务 3：funding rate 自动化

> 请设计 premium / index based funding rate generation，并与现有 funding_rates store 和 batch settlement 结合。

### 任务 4：读模型服务化

> 请把当前 pull-based projections 设计成 standalone projection service，包含 position、margin、pnl、audit、market data read store。

### 任务 5：一致性巡检

> 请设计 ledger / sequencer / matching / trade journal 的一致性校验工具，用于每日巡检和启动前检查。

### 任务 6：压测分析

> 请分析 10 worker 并发下锁竞争可能来自 matching、ledger 还是 risk，并给出 profiling 和优化计划。

---

## 22. 当前最重要结论

这个项目现在的状态不是“还没开始”，也不是“已经完整生产级”。

最准确判断：

> Rust Exchange 已经具备可信 v1 交易核心基础，可以进入受控部署和真实环境验证；后续重点应放在强平、funding、读模型、运维、审计、安全和长稳验证，而不是重写主交易链路。

