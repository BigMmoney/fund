# Rust Exchange 交易规则与风控状态机报告（2026-03-12）

## 1. 当前真实主链路

基于当前代码，`rust-exchange` 的真实写入主线已经固定为：

`API Gateway -> Principal/Auth -> Sequencer -> Risk Reserve/Check -> Partitioned Matching -> Ledger Settle -> Trade Journal/Snapshot -> Replay/Automation`

对应代码落点：

- API 入口与命令构造：`crates/api/src/main.rs`
- 鉴权与 body 完整性：`crates/api/src/security.rs`
- 启动恢复与调度：`crates/api/src/bootstrap.rs`
- 风控状态机：`crates/risk/src/lib.rs`
- 撮合状态机：`crates/matching/src/partitioned.rs`
- 账本最终结算：`crates/ledger/src/lib.rs`

这条链路当前已经不是概念设计，而是本地代码中的真实执行路径。

---

## 2. API / 鉴权状态机

### 2.1 请求进入条件

所有写接口当前都要求：

- 内部 principal 头存在且签名有效
- `x-request-id` 存在
- `x-internal-auth-body-sha256` 与真实 body 一致
- 路由级 user/admin 权限检查通过

这意味着写请求要经过如下状态：

`Unauthenticated -> SignatureVerified -> BodyIntegrityVerified -> RoleAuthorized -> CommandSequenced`

相关代码：

- `crates/api/src/security.rs:169`
- `crates/api/src/security.rs:280`

### 2.2 安全边界结论

当前签名已经绑定：

- method
- path
- query
- subject
- role
- session_id
- timestamp
- request_id
- body hash

因此现在的控制面/交易面，已经不再是“只校验头，不校验正文”的半签名模型。

---

## 3. Sequencer 状态机

API 层不会直接构造最终交易结果，而是先进入 `sequencer`：

- `sequence_new_order(...)`
- `sequence_replace_order(...)`
- 其他 cancel/admin 路径的 sequencing helper

Sequencer 的职责是：

- 分配单调 `command_seq`
- 持久化命令 WAL
- 为后续 replay 提供唯一命令源
- 维护生命周期 marker

生命周期更新 helper 当前位于：

- `crates/api/src/helpers.rs`

状态语义上可以理解为：

`Received -> Sequenced -> RiskReserved -> Routed -> PartitionAccepted -> Executed -> Settled -> Completed`

并不是每条命令都会经过全部节点，例如：

- 普通撤单通常直接走到 `Completed`
- 无成交的新单可能停在 `Active` 语义，不立即 `Completed`

---

## 4. 风控状态机

### 4.1 风控职责边界

当前 `risk` crate 已经承担以下真实职责：

- 现金/仓位预占与释放
- `reduce_only` 可卖容量校验
- 保证金快照计算
- 强平判定
- 部分强平执行
- ADL ranking
- socialized loss 分摊
- funding 结算

关键入口：

- `crates/risk/src/lib.rs:544` `ensure_reduce_only_sell_capacity(...)`
- `crates/risk/src/lib.rs:595` `margin_snapshot(...)`
- `crates/risk/src/lib.rs:650` `evaluate_liquidation(...)`
- `crates/risk/src/lib.rs:775` `execute_partial_liquidation_with_governance(...)`
- `crates/risk/src/lib.rs:1198` `adl_ranking_with_governance(...)`
- `crates/risk/src/lib.rs:1339` `apply_socialized_loss_with_governance(...)`

### 4.2 reduce-only 当前真实规则

当前并不是“只要余额非零就允许 reduce-only 卖单”。

实际逻辑是：

- 对 Spot：
  - 看可用持仓 + 已持仓 hold
- 对 Margin / Perpetual：
  - 看可用衍生品净头寸
- 再扣掉已保留给卖单的占用量

只有 `requested_amount <= remaining_capacity` 时才通过。

所以当前 `reduce_only` 更接近真实交易所风控，而不是原型级假判断。

### 4.3 强平状态机

当前强平逻辑的主状态是：

`Healthy -> MarginSnapshot -> LiquidationCandidate -> PartialLiquidation -> Penalty/Insurance/SocializedLoss -> Result`

其中：

- 先看 `margin_snapshot(...)`
- 再由 `evaluate_liquidation(...)` 产出候选
- 执行时进入 `execute_partial_liquidation_with_governance(...)`
- 若罚金不足，再经过 insurance / socialized loss / ADL 路径

这是当前代码的真实状态机，不是未来规划。

---

## 5. 撮合状态机

### 5.1 基本撮合规则

`partitioned` 撮合核心当前已经明确支持：

- 价格优先 / 时间优先
- kill switch
- market state 检查
- 价格带校验
- 自成交防护
- `replace` 原子语义
- 分区 snapshot / replay

关键落点：

- `crates/matching/src/partitioned.rs:482` `replay_command(...)`
- `crates/matching/src/partitioned.rs:2406` `match_incoming(...)`
- `crates/matching/src/partitioned.rs:2956` `cancel_orders(...)`

### 5.2 自成交防护

当前真实行为是：

- 检测到自成交时返回 `SubmissionError::SelfTradePrevented(...)`
- 默认不是隐式自成交撮合，也不是偷偷吃掉 resting order

因此当前策略更接近“拒绝 taker/阻止撮合”的保守安全路线。

### 5.3 replace 当前真实规则

当前 `replace` 已修到“显式 `cancel + new` 语义，但对外表现原子”：

- 先校验新单是否可接受
- 失败则旧单保留
- 成功才移除旧单并插入新单
- 优先级丢失

这一点在测试里也有覆盖：

- `replace_order_invalid_new_order_keeps_existing_order`
- `replace_order_risk_failure_keeps_existing_hold_and_order`
- `replace_order_loses_priority`

相关区域：

- `crates/matching/src/partitioned.rs:4075`
- `crates/matching/src/partitioned.rs:4144`
- `crates/matching/src/partitioned.rs:4204`

### 5.4 快照与恢复状态机

当前恢复不是“只看全局最大 snapshot seq”，而是：

- 每个 partition 记录自己的 `last_applied_command_seq`
- replay 时按命令目标分区判断是否需要重放

因此恢复状态机更接近：

`Load Snapshot Cursor(per partition) -> Read Sequencer WAL -> Route Command -> Replay if seq > partition_cursor`

这条边界是当前架构正确性的关键点之一。

---

## 6. 账本状态机

账本现在不是一个“余额数字 map”，而是 append-only WAL 驱动的会计状态机：

- validate delta
- verify sufficient balance
- apply entries
- bump versions
- dedupe `op_id`

Spot 成交结算已经通过 `SpotTradeSettlement` 收口：

- `crates/ledger/src/lib.rs:26`
- `crates/ledger/src/lib.rs:497`

这让账本接口比之前更明确：

- Spot 成交是一种专门结算语义
- 不再是“传一长串散参数”的弱边界

---

## 7. 自动化风控运行态

当前自动化并不是停留在设计图里，而是已经接入启动流程：

- liquidation scheduler
- liquidation worker scheduler
- funding scheduler

入口：

- `crates/api/src/bootstrap.rs:117`
- `crates/api/src/main.rs:1854`

所以当前运行态已经具备：

- 交易主链路
- 恢复主链路
- 自动化扫描/执行主链路

这也是为什么当前系统已经不能再被描述成“只是一个撮合 demo”。

---

## 8. 当前规则的业务含义

如果从业务/风控/审计共识的角度看，当前系统已经明确了以下规则：

- Rust 是唯一交易真相
- 所有写操作先认证、再完整性校验、再 sequencing
- 风控先于撮合入场
- 撮合失败不能把状态弄脏
- `replace` 必须是原子可预测的
- `reduce_only` 必须基于净头寸与已占用量
- 恢复必须按 partition cursor 判断，不能用全局近似

这些都已经是代码里能验证到的规则，而不是 README 口号。

---

## 9. 当前仍然属于“能力继续建设”，不是“当前逻辑错误”的部分

到这个阶段，剩下更像交易所级能力建设，而不是当前主链路 bug：

- 更复杂的 operator workflow
- 更细粒度治理审批流
- 更高阶价格仲裁策略
- 更复杂 liquidation auction book 机制
- 更完整的观测/指标/运维闭环

这些是下一阶段扩展，不影响当前“本地代码状态机已经闭环且可验证”的结论。

---

## 10. 一句话结论

当前 `rust-exchange` 的真实状态不是“还在想怎么设计交易所”，而是：

**交易写链路、风控链路、撮合链路、账本链路、恢复链路、自动化链路都已经在 Rust 主线上形成了可运行、可验证、可审计的状态机。**
