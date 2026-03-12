# 验证报告：多源价格仲裁 / 阶梯式清算簿 / 双人审批治理（2026-03-12）

## 1. 本轮目标

本轮验证聚焦 3 条新增或增强主线：

1. 多源 `index quorum / source degradation / fair-price arbitration`
2. 真正按数量逐步成交的 liquidation ladder / multi-bid auction book
3. 敏感控制面的 dual-approval / governance audit workflow

同时补做一次编译、格式化、测试回归，确认当前主线仍然闭环可用。

---

## 2. 已核验的代码点

### 2.1 多源指数仲裁

已确认 `PersistentIndexPriceStore` 不再是单值覆盖，而是以 source 为维度存储并在读取时仲裁。

已确认逻辑包含：

- stale source 过滤
- 中位数 baseline
- deviation bps outlier 剔除
- quorum 判断
- quorum 失败时 degraded fallback
- fair price 输出仲裁状态指标

关键位置：

- `crates/api/src/main.rs`
  - `ArbitratedIndexPrice`
  - `PersistentIndexPriceStore`
  - `fair_price_quote_for_snapshot(...)`
  - `resolve_mark_price_for_market(...)`

### 2.2 阶梯式 liquidation ladder

已确认清算 worker 从“单赢家一轮模型”升级为：

- 同一轮可遍历多个有效 bid
- 支持 partial fill
- `remaining_position_qty` 累计下降
- 轮次未完成时可进入下一轮
- 超过轮次预算后进入 retry tier / 更高层处置

关键位置：

- `crates/api/src/main.rs`
  - `run_liquidation_queue_worker(...)`

### 2.3 风险层部分清算

已确认 `crates/risk` 已支持带 `liquidation_qty` 的部分清算入口：

- `execute_liquidation_with_governance(...)`
- `execute_partial_liquidation_with_governance(...)`

这让 ladder 式清算可以按成交量逐步落账，而不是只能全量执行。

### 2.4 双人审批治理流

已确认敏感控制面写入已进入 `pending -> applied/rejected` 流程。

新增核心对象：

- `GovernanceActionRecord`
- `PendingGovernanceActionStore`
- `create_pending_governance_action(...)`
- `apply_governance_action(...)`

已确认以下动作不再由单管理员直接立即生效，而是先进入待审批：

- `adl_governance_update`
- `liquidation_policy_update`
- `index_price_upsert`
- `liquidation_queue_override`

已确认审批规则包括：

- 只有 admin 可操作
- 状态必须是 `pending`
- 请求人与审批人不能是同一 admin
- 审批成功后才真正 apply 到相应 store

相关接口：

- `GET /admin/risk/governance/actions`
- `POST /admin/risk/governance/actions/:action_id/approve`
- `POST /admin/risk/governance/actions/:action_id/reject`

---

## 3. 本轮额外收口

本次还做了一个小的代码清理：

- 删除 `PersistentIndexPriceStore::get(...)` 死代码，消除编译 warning
- 移除审批路由里几个无意义的占位读取语句，避免后续误解为业务依赖

这部分不改变业务语义，只是把当前实现收得更干净。

---

## 4. 执行的真实验证命令

在 `d:\pre_trading\rust-exchange` 执行：

- `cargo fmt --all`
- `cargo check -q`
- `cargo check -p api -q`
- `cargo test -q`

---

## 5. 结果

### 5.1 格式化

- `cargo fmt --all`：通过

### 5.2 编译

- `cargo check -q`：通过
- `cargo check -p api -q`：通过

### 5.3 测试

- `cargo test -q`：通过

测试输出显示当前 workspace 多个 crate 的测试集均通过，没有新增失败。

---

## 6. 当前结论

### 6.1 价格仲裁

当前已经从“单点人工价格写入”推进为“多源仲裁 + 降级模式”的风险可用模型，能够更真实地支撑 mark/fair price 相关逻辑。

### 6.2 清算拍卖

当前 liquidation 已不再是粗糙的一轮单中标模型，而是开始具备拍卖簿和阶梯式逐步处置特征，更接近交易所清算链路。

### 6.3 治理安全性

当前敏感风险配置和人工价格干预已经开始进入双人审批闭环，显著降低了单管理员直接改风险真相的风险。

---

## 7. 仍然存在但已知的边界

以下并不是本轮回归失败，而是当前架构仍需继续深化的已知边界：

1. 多源价格仲裁还没有 source reliability score / 动态权重 / 更复杂的 source health 模型。
2. liquidation ladder 虽已成立，但仍主要是 API orchestration 层实现，尚未完全抽成独立拍卖引擎 crate。
3. bankruptcy engine 还可以继续往真正的 mark/index/fair 统一模型推进。
4. operator override / liquidation retry workflow 仍可进一步做成更完整的治理工作流系统。
5. 风险读模型（Position/PnL/Margin projections）仍未完全独立为专门 projection crate。

---

## 8. 总评

截至 2026-03-12，本轮新增能力已经把系统从“带清算逻辑的撮合后端”进一步推进到：

**具备多源价格仲裁、阶梯式清算簿、双人审批治理流的交易风险后端骨架。**

从工程角度看，这轮代码目前处于：

- 可以编译
- 可以通过当前测试
- 逻辑方向正确
- 架构边界比之前更清晰

但距离最终交易所级完成态，仍需继续把价格治理、清算拍卖子系统、风险读模型、治理审计工作流继续做深。
