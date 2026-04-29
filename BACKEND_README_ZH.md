# Pre-Trading 后端真实代码结构说明（中文版）

本文只基于当前本地仓库中的后端代码真实结构整理，不包含前端内容，也不以历史设计稿为准。

## 1. 先说结论

当前仓库里实际并存两套后端实现：

- 一套较早期的 Go 原型服务，目录主要是 `api/`、`matching/`、`ledger/`、`risk/`、`indexer/`、`price-service/`、`hft-stream/` 等。
- 一套明显更完整、也是当前主后端形态的 Rust 工作区，目录在 `rust-exchange/`。

如果从代码规模、模块拆分、持久化能力、启动装配、接口数量、测试覆盖和运行时设计来看，`rust-exchange/` 才是当前仓库里真正的主后端实现；根目录下的 Go 服务更像早期兼容层、样例服务或原型残留。

## 2. 后端目录总览

### 2.1 根目录 Go 后端

根目录下可运行的 Go 后端目录主要有：

- `api/`
- `matching/`
- `ledger/`
- `risk/`
- `indexer/`
- `price-service/`
- `hft-stream/`
- `services/types/`
- `services/eventbus/`
- `services/utils/`

这一层的特点是：

- 基本以单个 `main.go` 为中心。
- 主要使用内存状态。
- 通过简单事件总线耦合。
- 适合演示、验证和兼容接口，不像最终生产主线。

### 2.2 Rust 主后端工作区

`rust-exchange/Cargo.toml` 定义了一个 workspace，成员包括：

- `crates/types`
- `crates/instruments`
- `crates/projections`
- `crates/eventbus`
- `crates/persistence`
- `crates/sequencer`
- `crates/ledger`
- `crates/risk`
- `crates/matching`
- `crates/api`

这是一个标准的“领域核心 + 基础设施 + API 装配层”的后端结构。

## 3. 当前主后端的真实分层

按真实代码职责，Rust 后端大致可以分成六层。

### 3.1 类型与规则层

对应目录：

- `rust-exchange/crates/types`

这一层是全系统共享的领域模型和规则定义，核心内容包括：

- 账户、台账、流水、订单、成交、命令等基础类型
- 市场状态机、撮合模式、订单类型、TIF、STP 等交易规则
- 品种定义 `InstrumentSpec`
- 保证金、清算、手续费、限频、期权、到期结算等规则类型
- API 错误码、命令生命周期、管理员动作

这层的作用不是“工具类”，而是整个后端的统一语言层，其他 crate 基本都依赖它。

### 3.2 基础设施层

对应目录：

- `rust-exchange/crates/persistence`
- `rust-exchange/crates/eventbus`
- `rust-exchange/crates/instruments`

职责分别是：

- `persistence`：提供 WAL 抽象 `WalStore<T>`，实现内存 WAL 和 JSONL 文件 WAL，支持 CRC 校验、轮转、group commit、best-effort 恢复。
- `eventbus`：提供进程内广播事件总线，基于 Tokio broadcast。
- `instruments`：提供品种注册中心，支持内存版和持久化版 `PersistentInstrumentRegistry`。

这一层解决的是“系统怎么存、怎么广播、怎么拿配置型元数据”，不直接承载交易业务。

### 3.3 核心事务层

对应目录：

- `rust-exchange/crates/sequencer`
- `rust-exchange/crates/ledger`
- `rust-exchange/crates/risk`
- `rust-exchange/crates/matching`

这层是后端主链路核心。

#### Sequencer

`sequencer` 负责命令排序和生命周期推进：

- 为请求分配单调递增 `command_seq`
- 记录 `request_id -> command` 映射
- 持久化 sequenced command 到 WAL
- 维护命令生命周期：`Received -> Sequenced -> WalAppended -> RiskReserved -> Routed -> PartitionAccepted -> Executed -> Settled -> Completed`

它本质上是写入路径的顺序控制器和幂等入口。

#### Ledger

`ledger` 是强约束资金账本，核心特征有：

- 双分录记账
- `op_id` 幂等
- WAL 落盘后再提交状态
- 全局余额守恒校验
- 允许部分系统账户或衍生品仓位账户出现受控负值
- 支持恢复、去重集合裁剪、并发分片锁

账本不是“成交后的附属品”，而是整个资金与仓位变化的落账核心。

#### Risk

`risk` 是交易前和交易后的风险引擎，职责明显比 Go 原型复杂很多：

- 订单预校验
- 保证金与抵押物计算
- 逐仓 / 全仓 / 多币种 / 组合保证金逻辑
- 强平判断
- ADL 候选计算
- 资金费结算
- 到期结算
- 压力测试与组合风险视图

它和 `ledger` 强绑定，因为很多风险计算最终要落到账户余额和仓位账户上。

#### Matching

`matching` 里同时保留了一个简单撮合引擎和一个更核心的分区撮合引擎，当前主线显然是：

- `PartitionedMatchingEngine`

其特点包括：

- 分区路由
- 每市场或每 outcome 的分配策略
- 队列容量与背压
- 订单簿维护
- 成交撮合
- STP 多策略
- 价格带约束
- CancelOnly / Halted 等市场状态控制
- 限速与账户冻结
- 快照、成交日志、结算记录落盘

这不是简单的单线程 order book，而是“带持久化、带风险挂钩、带运行时观测”的撮合核心。

### 3.4 投影与读模型层

对应目录：

- `rust-exchange/crates/projections`

这一层做只读派生，不直接修改主状态。主要输出：

- 持仓投影
- 保证金投影
- 未实现盈亏投影
- 持仓成本投影
- 资金费率投影
- 持仓量 / Open Interest 投影
- 费率与成交费汇总投影

这说明当前架构已经开始区分：

- 写模型：sequencer / matching / ledger / risk
- 读模型：projections

虽然还在同一进程内，但思路已经接近 CQRS。

### 3.5 API 装配与运行时层

对应目录：

- `rust-exchange/crates/api`

这是整个系统的对外入口和运行时拼装层，不是纯 Controller 层。它实际承担了：

- 启动配置加载与校验
- 各个持久化 store 初始化
- 核心组件 bootstrap
- Warp 路由装配
- REST / WebSocket 出口
- 自动化任务拉起
- 健康检查、可观测性、限流、鉴权、管理面

`api` crate 实际上是“应用层 + 接口层 + runtime orchestration”三者合一。

### 3.6 自动化与控制面层

这部分主要散落在 `crates/api/src` 下的多个模块里，例如：

- `admin.rs`
- `control.rs`
- `governance.rs`
- `liquidation.rs`
- `pricing.rs`
- `ops.rs`
- `release.rs`
- `rollback.rs`
- `capacity.rs`
- `oncall.rs`
- `sentinel.rs`
- `observability.rs`
- `prometheus.rs`
- `websocket.rs`

这说明系统已经不只是“撮合 API”，还内建了：

- 管理面
- 运维面
- 发布 / 回滚面
- 风险治理面
- 实时推送面
- 观测与告警面

## 4. 主后端的真实启动与装配顺序

从 `rust-exchange/crates/api/src/main.rs` 看，启动主链路大致是：

1. 初始化 tracing
2. 初始化内部鉴权密钥
3. 加载并校验配置 `ExchangeConfig`
4. 构建 Tokio runtime
5. 进入 `async_main`
6. 创建 `EventBus`
7. 执行 `bootstrap_runtime`
8. 得到核心运行对象：
   - `ledger`
   - `sequencer`
   - `risk`
   - `instruments`
   - `partitioned_engine`
   - 多种 WAL-backed store
9. 建立限流器、StopOrderStore、SystemSentinel
10. 校验账本全局不变量
11. 构建交易、控制、管理、账户、市场、转账、提现、治理、性能、运维、观测、WebSocket 等全部路由
12. 启动自动化后台任务
13. 建立 EventBus 到 WebSocket 的桥接
14. 启动 stop order 触发桥接
15. 启动 orderbook / mark price 周期推送
16. 启动 Warp HTTP 服务

这说明主后端不是“路由直接调几个函数”的轻应用，而是完整运行时。

## 5. 交易主链路怎么流动

结合 `sequencer`、`matching`、`risk`、`ledger` 的真实代码，可以把主写入路径概括为：

1. API 接收请求并做鉴权、限流、参数解析
2. 请求被封装为 `Command`
3. `Sequencer` 分配 `command_seq`，建立请求级幂等与生命周期管理
4. `RiskEngine` 进行预校验与准备金/保证金判断
5. `PartitionedMatchingEngine` 按市场路由到分区
6. 撮合引擎执行订单簿操作、STP、状态机判断、价格带检查等
7. 产生成交后，调用 `RiskEngine` / `LedgerService` 执行结算
8. 账本通过 `LedgerDelta` 原子提交
9. 相关记录写入 WAL：
   - sequencer WAL
   - ledger WAL
   - partition snapshot WAL
   - trade journal WAL
   - trade settlement WAL
   - 其他专用 store
10. 事件通过 `EventBus` 广播
11. WebSocket 和读接口基于快照、投影、日志结果对外输出

这条链路的关键特征是：

- 不是 DB 驱动，而是 WAL + 内存运行态驱动
- 不是单纯 REST CRUD，而是命令流驱动
- 不是只关心撮合，还把风控、清算、账本作为主链路一等公民

## 6. 当前后端的持久化设计

当前 Rust 主线并没有看到传统关系型数据库作为主状态源，真实持久化重心是 WAL 文件。

主要特点：

- 统一抽象：`WalStore<T>`
- 默认实现：`JsonlFileWal<T>`
- 每条记录追加写
- 支持 CRC 校验
- 支持恢复模式
- 支持 WAL 轮转
- 支持 group commit

主后端里有多类专用持久化 store，至少包括：

- instrument registry
- funding rate store
- liquidation queue store
- liquidation auction store
- ADL governance store
- liquidation policy store
- index price store
- position cost store
- governance action store
- sequencer WAL
- ledger WAL
- trade journal WAL
- trade settlement WAL
- partition snapshot WAL

这说明当前系统是“多份 append-only 日志 + 内存态重建”的架构，而不是“所有状态都直接写数据库表”。

## 7. API 层不是简单 MVC，而是模块化功能平面

`rust-exchange/crates/api/src` 下模块很多，按职责可分为几类。

### 7.1 交易与账户面

- `trading.rs`
- `accounts.rs`
- `markets.rs`
- `transfers.rs`
- `withdrawals.rs`
- `custody.rs`
- `stop_orders.rs`
- `position_costs.rs`
- `pricing.rs`
- `product_flows.rs`

### 7.2 管理与治理面

- `admin.rs`
- `control.rs`
- `governance.rs`
- `release.rs`
- `rollback.rs`

### 7.3 风险与清算面

- `liquidation.rs`
- `security.rs`
- `sentinel.rs`

### 7.4 运行时与运维面

- `ops.rs`
- `capacity.rs`
- `oncall.rs`
- `perf.rs`
- `stress.rs`
- `failpoint.rs`

### 7.5 可观测与接口描述

- `observability.rs`
- `prometheus.rs`
- `openapi.rs`
- `websocket.rs`
- `tracing_ctx.rs`
- `planes.rs`

换句话说，当前 API 层更像一个“统一接入壳 + 多控制平面系统”，而不是传统三层架构里的薄 Controller。

## 8. 根目录 Go 后端真实定位

根目录 Go 代码不能说没用，但从真实结构上看，它更像早期阶段的原型或兼容壳。

### 8.1 Go API Gateway

`api/main.go` 的实际特征是：

- 用 Gorilla Mux 和 WebSocket 提供 HTTP / WS
- 带基础 CORS、鉴权、限流
- 一部分接口直接代理到 Rust core
- 另一部分仍保留 demo / compatibility 内存逻辑
- 内部还有 markets、users、orderBooks、trades 等内存态数据

这意味着它并不是系统最终业务核心，而更像：

- 兼容入口
- 演示网关
- 旧接口保留层

### 8.2 Go Matching / Ledger / Risk

这几个目录的特点很一致：

- 基本都是单文件主程序
- 内存态实现
- 使用 `services/eventbus` 做进程内消息
- 功能正确但抽象层级较浅

例如：

- `matching/main.go`：批量撮合、计算清算价、按比例分配成交
- `ledger/main.go`：双分录账本、幂等 `op_id`、简单 WAL 切片
- `risk/main.go`：市场状态、kill switch、动态风险参数

它们能表达概念，但和 Rust 主线相比，工程化程度明显低很多。

## 9. 当前代码呈现出的后端设计风格

如果不参考任何设计文档，只看真实代码，可以总结出当前后端的设计风格是：

- 以 Rust workspace 为核心的模块化单体
- 以内存运行态 + WAL 持久化为主
- 以命令流和顺序号驱动写入链路
- 以撮合、风险、账本三核协同为主线
- 通过 projection 做读模型派生
- 在 API 层集中装配控制面、治理面、观测面和自动化任务
- 保留一套 Go 兼容/原型层，但主干已迁往 Rust

它不是典型微服务系统，也不是传统 MVC 单体，更接近：

- “高性能交易内核 + 单进程多模块控制面”的应用架构

## 10. 如果按真实代码给出一版分层命名

可以把当前后端整理成下面这套更贴近代码的命名：

### L0 统一领域类型层

- `crates/types`

### L1 基础设施层

- `crates/persistence`
- `crates/eventbus`
- `crates/instruments`

### L2 核心事务引擎层

- `crates/sequencer`
- `crates/ledger`
- `crates/risk`
- `crates/matching`

### L3 读模型 / 投影层

- `crates/projections`

### L4 应用装配与接口层

- `crates/api`

### L5 兼容原型层

- 根目录 Go 服务：`api/`、`matching/`、`ledger/`、`risk/` 等

## 11. 最终判断

基于当前本地代码，最准确的后端判断应该是：

- 当前真实主后端是 `rust-exchange/`
- 根目录 Go 服务属于旧实现、兼容层或原型遗留
- 系统已经形成较清晰的“类型 -> 基础设施 -> 核心事务 -> 投影 -> API 装配”分层
- 写路径以 sequencer、matching、risk、ledger 为核心
- 持久化以多类 WAL 为中心
- 运行时能力已扩展到治理、强平、ADL、观测、WebSocket、发布回滚、容量管理等多个控制平面
。
