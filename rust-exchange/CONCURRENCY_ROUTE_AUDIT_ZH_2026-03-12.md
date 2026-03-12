# API 路由拆分与并发审查报告（2026-03-12）

## 1. 本轮完成内容

本轮不是继续加新交易语义，而是做两类收口：

1. 把 `crates/api/src/main.rs` 中最重的控制面路由按域拆开，降低主文件耦合度。
2. 对当前 Rust 主线的锁、线程、竞争和共享状态边界做一次工程化审查，确认“现在是否基本合理”。

---

## 2. 路由层拆分结果

### 2.1 已拆出的模块

- `crates/api/src/pricing.rs`
  - 多源 index price store
  - fair price 计算
  - pricing 管理路由 builder
- `crates/api/src/governance.rs`
  - governance action store
  - dual approval apply/create 逻辑
  - governance 管理路由 builder
- `crates/api/src/liquidation.rs`
  - liquidation queue override
  - liquidation worker
  - liquidation / auction / insurance fund 路由 builder

### 2.2 主文件现在承担的职责

`crates/api/src/main.rs` 现在主要负责：

- 通用请求/响应类型定义
- store / engine / registry 初始化
- 通用 filter、auth、限流、错误处理
- 主路由装配
- 调度器启动

这意味着它已经从“既写业务逻辑、又写控制面逻辑、又写所有路由细节”的状态，进一步收口成“应用装配器”。

### 2.3 当前收益

这次拆分最直接的收益是：

- pricing / governance / liquidation 的边界更清楚
- 后续继续把控制面拆 crate 时，迁移成本更低
- 主文件发生逻辑污染的风险下降
- 风险审查和 code review 时可以按领域看文件，而不是在 1 个超长 `main.rs` 里追

---

## 3. 并发与共享状态审查

### 3.1 API 层共享状态

当前 API 层的共享状态主要是以下几类：

1. `DashMap` 持有高频读写的内存索引
2. `Mutex` 串行化需要强一致追加的状态
3. `Arc` 做跨 route / scheduler / worker 共享
4. append-only WAL store 做持久化落点

这条路线总体是合理的，因为控制面与风险状态不是纯计算缓存，而是需要：

- 容错
- 审计
- 追加顺序
- 可恢复

### 3.2 看起来合理的点

#### A. 撮合主线仍保持“分区内串行，分区间并行”

撮合引擎最关键的正确性仍由 `matching::partitioned` 保证：

- 每个分区通过 `tokio::mpsc` 串行消费命令
- 用 `AtomicUsize` 跟踪 inflight / dirty 计数
- 用 `AtomicBool` 控制 kill-switch
- 用 `oneshot` 回传命令执行结果

这是一种偏保守但正确的设计：

- 不追求订单簿内细粒度锁并发
- 优先确保单市场状态机不被并发写穿

对于当前阶段，这是正确的取舍。

#### B. 清算队列与拍卖簿使用 `write_lock + DashMap`

清算队列和拍卖簿本身既要支持快速读，也要保证某些写操作的顺序性，所以使用：

- `DashMap` 提供并发索引
- `Mutex<()>` 保护关键写路径顺序

这种模式虽然不是极致高吞吐，但对当前控制面/清算面是合理的，原因是：

- 清算不是每毫秒几十万次的主撮合热路径
- 它更关心状态不要乱序、不要丢 WAL、不要双写污染

#### C. 风险与治理 store 偏向“当前快照 + WAL append”

`adl governance`、`liquidation policy`、`governance action` 等 store 采用：

- 内存 current / entries
- 每次变更 append 到 WAL

这非常适合治理与审计场景，因为它天然保留了：

- 最近状态
- 历史决策轨迹
- 恢复所需最小事实链

### 3.3 当前值得注意但还不构成阻断的问题

#### A. API 仍是较强 orchestrator

虽然路由层已经开始拆分，但 `api` crate 仍然承担过多编排职责：

- 取 snapshot
- 算 fair price
- 驱动 liquidation worker
- 管理审批流
- 启动 automation scheduler

这不是线程安全 bug，但它会导致：

- 以后改一个控制面动作，容易牵到太多依赖
- 审计与测试粒度不够细
- 共享状态的依赖图继续膨胀

#### B. 控制面很多操作还是“跨多个 store 协调”

比如治理审批成功后，可能会影响：

- governance action store
- pricing store
- liquidation queue
- policy store

这类操作当前已经有审批与持久化，但还没有完全抽成更正式的 workflow transaction 边界。

换句话说：

- 当前逻辑基本正确
- 但“跨 store 编排”仍是以后最值得继续降复杂度的点

#### C. API scheduler 与 route 共处一体运行时

现在 liquidation / funding scheduler 是直接在 API 进程内 `tokio::spawn` 出来运行的。

优点：

- 部署简单
- 联调方便
- 当前阶段足够直接

缺点：

- 控制面流量、HTTP 压力、自动化任务共用同一 runtime
- 极端情况下更难精细隔离资源
- 后续如果 worker 负载继续上升，可能需要独立进程化

这不是当前 bug，但属于未来扩展性边界。

---

## 4. 线程 / 锁 / 竞争的结论

### 4.1 当前判断

截至本轮代码状态，当前 Rust 主线在“锁、线程、竞争、占用是否合理”这个问题上的结论是：

**基本合理，且当前设计偏正确性优先，没有看到新的明显脏竞争问题。**

更具体地说：

- 热路径撮合没有退化成大量共享锁竞争
- 风险/治理/清算控制面没有暴露出明显未受控并发写穿
- API 控制面使用 `Arc + DashMap + Mutex + WAL append` 的方式，在现阶段是稳妥的
- 现有结构更像“安全保守型交易后端”，而不是“冒险的高并发花活实现”

### 4.2 仍需继续关注的 3 个方向

1. **继续把控制面 orchestration 降到独立模块/子系统**
   - 重点是 liquidation governance workflow 和 automation scheduler。
2. **继续把读模型投影独立化**
   - Position / PnL / Margin 不应长期挂在 API 聚合逻辑上。
3. **未来把 automation worker 独立进程化**
   - 当清算/资金费率/治理任务变重时，独立运行时会更稳。

---

## 5. 本轮验证

实际执行：

- `cargo fmt --all`
- `cargo check -p api`
- `cargo test -q`

结果：全部通过。

---

## 6. 总结

这轮结束后，系统状态可以总结为：

- pricing / governance / liquidation 三块控制面已经从主文件拆成独立模块
- 主文件更接近装配层，而不是继续膨胀的业务大杂烩
- 当前并发模型没有发现新的明显逻辑级竞争问题
- Rust 主线仍然保持“分区串行、跨分区并行、控制面保守一致”的健康方向

结论上，当前代码已经比之前更接近“可长期演进的交易后端结构”，而不是单文件堆叠实现。
