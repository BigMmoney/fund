# API Bootstrap / App Context 收口审计报告（2026-03-12）

## 1. 本轮目标

在上一轮已经把 `store wiring` 从 `crates/api/src/main.rs` 拆到 `crates/api/src/stores.rs` 之后，
本轮继续处理剩余的“应用入口级耦合”：

- WAL 初始化
- ledger / sequencer / matching engine 启动
- snapshot restore + partition-aware replay
- automation scheduler 启动

目标不是改交易语义，而是把 `main.rs` 真正收口成“HTTP 入口 + 顶层装配”文件。

---

## 2. 本轮实际改动

新增：

- `crates/api/src/bootstrap.rs`

新增结构：

- `AppBootstrap`
- `AutomationRuntime`

迁出的方法：

- `bootstrap_runtime()`
- `spawn_automation_tasks()`
- `replay_commands_after_snapshot()`

本轮之后，`main.rs` 不再直接展开以下细节：

- 各类 WAL 文件初始化
- ledger / sequencer 恢复
- partitioned matching engine 初始化
- 分区级 snapshot 游标计算
- sequencer WAL 的 replay 边界处理
- liquidation / funding automation 的调度任务启动

---

## 3. 当前 API 分层实情

截至当前，`crates/api/src/` 已形成更清晰的层次：

### 3.1 路由层

- `trading.rs`
- `control.rs`
- `accounts.rs`
- `markets.rs`
- `admin.rs`
- `pricing.rs`
- `governance.rs`
- `liquidation.rs`

### 3.2 类型层

- `dto.rs`

### 3.3 启动/存储辅助层

- `stores.rs`
- `bootstrap.rs`

### 3.4 顶层入口层

- `main.rs`

这意味着 API 的文件职责，已经从“大文件堆叠”进入“入口编排 + 模块装配”的正常结构。

---

## 4. 架构价值

### 4.1 为什么 `bootstrap.rs` 值得单独存在

因为启动恢复链路本身就是一条独立架构路径：

`WAL open -> recover ledger -> recover sequencer -> restore matching snapshot -> replay sequenced commands by partition -> expose runtime`

这条路径既不属于路由层，也不属于 DTO 层，更不应该长时间混在 `main.rs` 里。

把它独立出来之后：

- `main.rs` 更容易审计
- replay 边界逻辑更容易单独检查
- 后续如果补 `AppContext`、配置注入、启动指标，也有明确落点

### 4.2 为什么 `AutomationRuntime` 要独立出来

liquidation / funding scheduler 启动时需要很多 runtime handle：

- engine
- risk
- instruments
- index price store
- liquidation queue / auction
- audit store

如果继续散落在 `main.rs`，可读性会快速下降，而且一旦新增自动化任务，就会再次把入口文件撑大。

现在这部分已经有了独立边界。

---

## 5. 正确性审计

### 5.1 本轮没有改变的语义

本轮是结构收口，不是业务重写；以下语义保持不变：

- sequencer WAL 恢复顺序
- snapshot + replay 的分区判定逻辑
- matching engine 初始化参数
- instrument registry 初始化逻辑
- liquidation / funding scheduler 的启动条件

### 5.2 本轮特别关注的恢复边界

恢复逻辑仍然保持“按目标分区判断 replay”的正确策略，而不是回退到全局最大/最小游标近似：

- 先导出每个 partition 的 `last_applied_command_seq`
- 再读取 sequencer 最新记录
- 仅当命令目标分区的 snapshot 游标落后时，才重放该命令

这条边界对于避免慢分区丢命令非常关键，本轮迁移后仍然保留在单独 helper 中，便于后续继续审查。

---

## 6. 并发与锁审计结论

本轮没有新增交易路径锁竞争：

- 没新增新的共享可变全局状态
- 没改变 matching / risk / ledger 的锁顺序
- 没改变分区线程模型
- 没把恢复逻辑改成并行乱序执行

因此，本轮主要改善的是**代码结构风险**，不是并发行为本身。

换句话说：

- 可读性下降风险降低了
- 启动链路误改风险降低了
- 交易态和恢复态的边界更容易继续审计了

---

## 7. 当前 `main.rs` 还剩什么

现在 `main.rs` 主要保留：

1. shared helper / auth / rejection / normalization
2. route builder 组合
3. CORS / static files / recover
4. HTTP bind / shutdown

这已经比较接近一个合理的应用入口文件。

---

## 8. 仍然存在的剩余耦合

虽然本轮已经明显收口，但仍有两类下一阶段候选项：

### A. shared helper 仍在 `main.rs`

例如：

- principal 提取与 guard
- rejection 映射
- lifecycle helper
- response normalization helper

这些可以继续下沉到：

- `auth_helpers.rs`
- `rejections.rs`
- `lifecycle.rs`
- `response_helpers.rs`

### B. DTO 仍是单文件

`dto.rs` 目前虽然已经独立，但如果继续膨胀，后续可以按域拆成子模块。

这已经不是“必须立即修”的耦合，而是下一轮整洁性优化。

---

## 9. 验证

本轮实际验证：

- `cargo fmt --all`
- `cargo check -p api`

在本轮报告落盘前，结构改动已通过编译校验。

建议最终交付前继续执行：

- `cargo test -q`

---

## 10. 审计结论

当前 API 入口已经从：

`单一 main.rs 同时承担路由 + DTO + stores + 启动恢复 + 自动化调度`

演进为：

`route modules + dto module + stores module + bootstrap module + clean main entry`

这说明本轮不是“写了更多文件”，而是真正把运行时边界收拢到了可长期维护的结构上。

如果继续推进，下一刀最合适的是 shared helper 下沉，而不是再动交易真相链路本身。
