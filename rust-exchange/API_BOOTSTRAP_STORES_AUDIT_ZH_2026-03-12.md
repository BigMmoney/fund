# API Bootstrap / Stores 审计报告（2026-03-12）

## 1. 本轮做了什么

本轮继续把 `crates/api/src/main.rs` 里的“应用装配细节”往外拆，重点处理的是最机械、最适合模块化的一层：`store wiring`。

新增：

- `crates/api/src/stores.rs`

已迁出的内容：

- `build_funding_rate_store()`
- `build_risk_automation_audit_store()`
- `build_liquidation_queue_store()`
- `build_liquidation_auction_store()`
- `build_adl_governance_store()`
- `build_liquidation_policy_store()`
- `build_index_price_store()`
- `build_governance_action_store()`
- `build_instrument_registry()`

这一步的价值很明确：

- `main.rs` 不再承担全部 domain store 初始化细节
- store 初始化边界开始与 route/bootstrap 分离
- 后续要继续抽 `AppContext` / `bootstrap.rs` 时，风险更低

---

## 2. 当前 API 结构实情

截至本轮，`crates/api/src/` 已基本形成以下层次：

### 2.1 路由层

- `trading.rs`
- `control.rs`
- `accounts.rs`
- `markets.rs`
- `admin.rs`
- `pricing.rs`
- `governance.rs`
- `liquidation.rs`

### 2.2 类型层

- `dto.rs`

### 2.3 store/bootstrap 辅助层

- `stores.rs`

### 2.4 顶层装配层

- `main.rs`

这说明 API 已经不再是一个“所有东西都塞在 main.rs”的实现，而是开始出现真正的层次结构。

---

## 3. 本轮之后 `main.rs` 还剩什么

当前 `main.rs` 仍然主要负责：

1. 通用 helper / error / auth / rate limit / normalization
2. WAL 初始化与 ledger / sequencer / matching engine 启动
3. snapshot restore + replay bootstrap
4. 顶层 route builder 装配
5. CORS / static files / recover
6. scheduler 启动

也就是说，`main.rs` 现在确实更像应用入口文件，而不是全业务文件。

---

## 4. 审计结论

### 4.1 已明显改善的部分

- store wiring 已经从主文件剥离一层
- route wiring 已经模块化
- DTO 已经统一模块化
- 编译和测试在持续重构后仍保持通过

这说明当前重构并不是“拆着拆着就坏”，而是已经形成稳定节奏。

### 4.2 还剩的主要耦合点

现在 API 层最主要的剩余耦合已经进一步缩小为 2 类：

#### A. bootstrap / recovery 仍集中在 `main.rs`

包括：

- ledger WAL 初始化
- sequencer WAL 初始化
- matching snapshot / trade journal WAL 初始化
- partition replay boundary 恢复

这部分已经不再是路由问题，而是“应用启动编排”问题。

#### B. shared helper 仍在 `main.rs`

包括：

- principal guard
- request normalization
- lifecycle 更新 helper
- rejection helper
- 统计/投影转换 helper

这些如果继续抽出去，会让 `main.rs` 更接近一个真正干净的入口文件。

---

## 5. 风险判断

### 5.1 正确性风险

本轮没有看到新增的逻辑级风险：

- store builder 迁移不改变 store 语义
- instrument registry 初始化逻辑未改变
- 当前 recovery / route / scheduler 逻辑保持原样

### 5.2 并发风险

本轮也没有引入新的并发风险：

- 没新增共享可变状态
- 没重排关键锁顺序
- 没修改 matching / liquidation 的线程模型

因此本轮是一次**低风险结构整理**，不是语义重构。

---

## 6. 推荐下一步

如果继续做，最值得的下一刀是：

### Step 1: 抽 `bootstrap.rs`

把以下内容从 `main.rs` 挪出去：

- WAL 初始化
- ledger/sequencer 初始化
- matching engine 初始化
- snapshot restore + replay

### Step 2: 抽 shared helpers

例如：

- `auth_helpers.rs`
- `request_helpers.rs`
- `rejection.rs`
- `lifecycle_helpers.rs`

### Step 3: DTO 分目录

把 `dto.rs` 继续细分为：

- `dto/trading.rs`
- `dto/control.rs`
- `dto/markets.rs`
- `dto/accounts.rs`
- `dto/risk.rs`

---

## 7. 验证

本轮实际执行：

- `cargo fmt --all`
- `cargo check -p api`
- `cargo test -q`

结果：全部通过。

---

## 8. 一句话总结

本轮完成后，`api` 已经从“路由拆分阶段”进一步推进到“store wiring 也开始脱离主文件”的状态。

这说明当前 `rust-exchange` 的 API 架构，已经进入可以继续做 `bootstrap/app context` 级别整理的阶段，而不是还停留在大文件止血阶段。
