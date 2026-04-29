# 状态机真相：关键操作的原子边界

> 本文档记录系统中 7 个关键操作的精确状态机语义。
> 每一步标注：输入状态 → WAL 记录点 → 内存变更点 → 失败处理 → 重放规则。

---

## 全局生命周期枚举

```
Received → Sequenced → WalAppended → RiskReserved → Routed
         → PartitionAccepted → Executed → Settled → Completed
                               ↘ Cancelled
                               ↘ Rejected
```

合法转换定义于 `crates/sequencer/src/lib.rs:is_valid_transition()`。
终态（Completed / Cancelled / Rejected）不可再转换。

---

## 1. 下单（New Order）

### 输入状态
- `Command::NewOrder` with `request_id ≠ ""`
- 用户账户存在且有足够余额
- 市场处于 `Normal` 或 `Stress` 状态

### 状态流转

| 步骤 | 操作 | WAL 记录点 | 内存变更点 |
|------|------|-----------|-----------|
| 1 | `sequencer.sequence_and_append()` | ✅ 写入 `SequencedCommandRecord{seq=N, lifecycle=WalAppended}` | `next_seq += 1`; `record_by_request.insert(req_id)` |
| 2 | `validate_new_order()` | ❌ 无 | 无（纯校验） |
| 3 | `validate_order_acceptance()` | ❌ 无 | 无 |
| 4 | `reserve_order_reservation()` | ❌ 无 | Risk engine 冻结保证金/现金 |
| 5 | `match_incoming()` | ✅ 在内部写入 trade journal / settlement WAL | 订单簿插入/撮合成交 |
| 6 | `release_order_reservation()` | ❌ 无 | 释放剩余预留 |
| 7 | `advance_replay_cursor(seq)` | ❌ 无 | `replay_cursor.last_applied_seq = N` |

### 失败处理

| 故障点 | 后果 | 恢复机制 |
|--------|------|---------|
| Step 1 WAL 写入失败 | 序列号回滚（`fetch_sub(1)`），命令未注册 | 客户端重试，新 request_id |
| Step 4 风控预留失败 | 返回 `SubmissionError`，已写入的 WAL 记录停留在 `WalAppended` | 重放时跳过（`should_skip_replayed_command`） |
| Step 5 撮合失败 | 已成交部分保留，未成交部分退回 | 部分成交结果返回，订单标记为 `PartiallyFilled` |
| Step 5 结算 WAL 失败 | **不回退订单簿**，市场自动暂停（`MarketState::Halted`） | 人工干预或恢复后重放 |

### 重放规则
- 重放时 `should_skip_replayed_command(seq)` 检查：若 `seq ≤ replay_cursor.last_applied_seq`，直接返回空结果
- 重放不重新执行风控预留（已包含在快照中）
- 重放不重复触发条件单

---

## 2. 撮合（Matching）

### 输入状态
- 订单已通过验证和风控
- 订单簿中已有对手方挂单
- `MarketState ∈ {Normal, Stress}`

### 状态流转

| 步骤 | 操作 | WAL 记录点 | 内存变更点 |
|------|------|-----------|-----------|
| 1 | `match_incoming()` 遍历订单簿 | ❌ 无 | 逐笔成交，更新双方剩余量 |
| 2 | 对每笔成交：`emit_fill_event()` | ❌ 无（eventbus 内存） | EventBus 推送 Fill 事件 |
| 3 | 写入 trade journal WAL | ✅ `TradeJournalRecord` | `seen_trade_ids.insert(trade_id)` |
| 4 | 写入 settlement WAL | ✅ `TradeSettlementRecord` | `settlement_statuses.insert(trade_id)` |
| 5 | 写入 position cost WAL | ✅ `PositionCostRecord` | 成本缓存更新 |
| 6 | 更新订单状态 | ❌ 无 | `order.remaining_amount -= fill.amount` |
| 7 | 若完全成交：`lifecycle → Completed` | ❌ 无 | 订单从簿中移除 |
| 8 | 若部分成交且可挂单：`insert_resting_order()` | ❌ 无 | 订单加入价格-时间队列 |

### 失败处理

| 故障点 | 后果 | 恢复机制 |
|--------|------|---------|
| Step 3 trade journal 写入失败 | 撮合中止，错误冒泡 | 重放时重新执行撮合 |
| Step 4 settlement 写入失败 | **市场暂停**（`MarketState::Halted`），已成交部分保留 | 修复 WAL 后重放 |
| Step 5 position cost 写入失败 | 同上 | 同上 |
| 自成交检测到 | 阻止成交，返回 `SelfTradePrevented` | 订单按 STP 模式处理 |

### 重放规则
- 通过 `seen_trade_ids` 去重：已记录的 trade_id 不重复结算
- 重放时使用 `skip_replayed_command` 跳过整个命令
- 快照恢复时订单簿从 `PartitionStateSnapshot` 重建

---

## 3. 结算（Settlement）

### 输入状态
- 撮合已产生成交（Fill）
- `SettlementRecord` 已写入 WAL
- 买卖双方账户存在

### 状态流转

| 步骤 | 操作 | WAL 记录点 | 内存变更点 |
|------|------|-----------|-----------|
| 1 | 从 match_incoming 调用 `ledger.settle_trade()` | ❌ | 无 |
| 2 | `ledger.commit_delta(LedgerDelta{op_id})` | ✅ `LedgerDelta` 写入 ledger WAL | `seen_op_ids.insert(op_id)` |
| 3 | `apply_entries()` | ❌ | 买卖双方余额更新 |
| 4 | `bump_versions()` | ❌ | 账户版本号 +1 |
| 5 | `event_bus.publish(LedgerCommitted)` | ❌ | 异步通知 |
| 6 | `lifecycle → Settled` | ❌ | sequencer 状态推进 |

### 失败处理

| 故障点 | 后果 | 恢复机制 |
|--------|------|---------|
| Step 2 ledger WAL 写入失败 | 返回错误，撮合已成交但资金未转移 | 重放时 ledger 通过 `seen_op_ids` 检测重复 |
| Step 3 余额不足 | 理论上不应发生（风控已预扣） | 市场暂停，人工介入 |
| 重复 op_id | `commit_delta_if_absent` 返回 `false` | 幂等处理，视为成功 |

### 重放规则
- Ledger 通过 `op_id` 幂等：相同 op_id 的 delta 只应用一次
- 恢复时 `recover_from_wal()` 重建所有账户余额
- **全局余额不变量**：所有账户余额之和必须为 0，违反则恢复失败

---

## 4. 撤单（Cancel Order）

### 输入状态
- 订单存在于订单簿中（`OrderState = Active` 或 `PartiallyFilled`）
- `Command::CancelOrder` with valid `order_id`

### 状态流转

| 步骤 | 操作 | WAL 记录点 | 内存变更点 |
|------|------|-----------|-----------|
| 1 | `sequencer.sequence_and_append()` | ✅ `SequencedCommandRecord` | `next_seq += 1` |
| 2 | 查找订单所在分区 | ❌ 无 | 通过 `order_id` hash 定位 |
| 3 | `cancel_orders()` | ❌ 无 | 从订单簿移除 |
| 4 | `release_order_reservation()` | ❌ 无 | 释放冻结的保证金/现金 |
| 5 | `update_lifecycle_after_cancel()` | ✅ sequencer WAL append `Cancelled` | `lifecycle → Cancelled` |
| 6 | `advance_replay_cursor(seq)` | ❌ 无 | `replay_cursor.last_applied_seq = N` |

### 失败处理

| 故障点 | 后果 | 恢复机制 |
|--------|------|---------|
| Step 1 WAL 失败 | 序列号回滚，撤单未注册 | 客户端重试 |
| Step 3 订单不存在 | 返回 `OrderNotFound` | 无需恢复 |
| Step 5 lifecycle WAL 失败 | **静默记录警告**（已修复），订单已取消但状态未持久化 | 重放时重新标记 |

### 重放规则
- 撤单是幂等的：重复撤同一订单 → `OrderNotFound`
- 重放时若订单已被后续操作消费，跳过

---

## 5. 触发（Trigger — Stop Loss / Take Profit）

### 输入状态
- 条件单已存入 `trigger_orders` 映射
- 市场价格变动触及触发价
- `MarketState ∈ {Normal, Stress}`

### 状态流转

| 步骤 | 操作 | WAL 记录点 | 内存变更点 |
|------|------|-----------|-----------|
| 1 | 成交后调用 `extract_triggered_commands()` | ❌ 无 | 扫描 `trigger_orders` |
| 2 | `is_trigger_met()` 判定 | ❌ 无 | 比较 last_price/mark_price vs trigger_price |
| 3 | 从 `trigger_orders` 移除 | ❌ 无 | `market.trigger_orders.remove(id)` |
| 4 | 转换为普通 `NewOrderCommand` | ❌ 无 | 清除 `trigger_price/trigger_type` |
| 5 | 递归调用 `process_new_order()` | ✅ 走完整下单流程 | 进入撮合引擎 |

### 失败处理

| 故障点 | 后果 | 恢复机制 |
|--------|------|---------|
| Step 2 未触发 | 继续等待下次价格变动 | 无操作 |
| Step 5 激活失败 | `tracing::warn!` 记录，触发单已丢失 | ⚠️ **当前无法恢复** — 触发单一旦从映射移除即丢失 |

### 重放规则
- 触发单不单独写 WAL，依赖快照恢复
- 快照中包含 `trigger_orders` 的完整状态
- 恢复后需等待新的价格变动才能重新触发

---

## 6. 清算（Liquidation）

### 输入状态
- 用户保证金比率低于维持保证金要求
- 市场未处于 `Halted/CancelOnly/Closed/Maintenance`
- 系统哨兵允许自动清算

### 状态流转

| 步骤 | 操作 | WAL 记录点 | 内存变更点 |
|------|------|-----------|-----------|
| 1 | `run_liquidation_cycle()` 扫描候选 | ❌ 无 | 计算保证金比率 |
| 2 | 组合级偿付能力预过滤 | ❌ 无 | 跳过有对冲的用户 |
| 3 | `initiate_liquidation_queue()` | ✅ `LiquidationQueueRecord` | 队列中添加待清算头寸 |
| 4 | `open_liquidation_auction()` | ✅ `LiquidationAuctionRecord` | 拍卖簿建立 |
| 5 | 阶梯式拍卖执行 | ✅ 每次出价 `LiquidationAuctionRecord` | 更新最优报价 |
| 6 | ADL 自动减仓（如拍卖失败） | ✅ `AdlExecutionRecord` | 强制平仓 |
| 7 | 清算完成结算 | ✅ 最终快照 | 头寸清零，余额调整 |

### 失败处理

| 故障点 | 后果 | 恢复机制 |
|--------|------|---------|
| Step 3 队列 WAL 失败 | ⚠️ **仅记录警告**（已修复） | 下次周期重新扫描 |
| Step 4 拍卖 WAL 失败 | ⚠️ **仅记录警告**（已修复） | 拍卖可能重复开启 |
| Step 5 拍卖出价 WAL 失败 | ⚠️ **仅记录警告** | 出价可能丢失 |
| Step 6 ADL 执行失败 | 同上 | 头寸持续暴露 |

### 重放规则
- 通过 `queue_id` 去重：相同 queue_id 的清算不重复发起
- 拍卖通过 `auction_id` 标识状态
- 恢复后从最新 WAL 记录重建拍卖状态
- 清算周期是定时任务，非幂等但可安全重入

---

## 7. 资金费率结算（Funding）

### 输入状态
- 永续合约市场（`InstrumentKind::Perpetual`）
- 距上次资金结算已过 `funding_interval_secs`
- 存在多空双方持仓

### 状态流转

| 步骤 | 操作 | WAL 记录点 | 内存变更点 |
|------|------|-----------|-----------|
| 1 | `run_funding_cycle()` 获取快照 | ❌ 无 | 无 |
| 2 | 确定资金费率（手动或推导） | ❌ 无 | 无 |
| 3 | `settle_funding_batch()` 配对多空 | ❌ 无 | 无 |
| 4 | 对每对：`settle_funding_between_users()` | ❌ | 无 |
| 5 | `ledger.transfer_cash()` | ✅ `LedgerDelta{op_id}` | 付款方→收款方余额转移 |
| 6 | 更新 `last_funded` 时间戳 | ❌ 无 | `last_funded.insert(key, now)` |

### 失败处理

| 故障点 | 后果 | 恢复机制 |
|--------|------|---------|
| Step 4 配对失败 | 跳过该对，继续处理 | `Err(_)` 被 catch，batch 继续 |
| Step 5 ledger transfer 失败 | 该对资金费未结算 | 通过 `op_id` 幂等，下次周期可重试 |
| Step 6 时间戳未更新 | 下个周期可能重复结算 | 通过 `op_id` 幂等保护 |

### 重放规则
- 每笔资金转移使用唯一 `op_id`（格式：`auto-funding:pair-N`）
- Ledger 的 `commit_delta_if_absent` 保证幂等
- 恢复后 `last_funded` 从内存丢失，需要重新判断间隔
- 资金费率本身不写 WAL，每次从指数价格推导或手动覆盖

---

## 跨操作不变量

| 不变量 | 检查点 | 违反后果 |
|--------|--------|---------|
| 全局余额为零 | `ledger.recover_from_wal()` | 恢复失败，拒绝启动 |
| 序列号单调递增 | `sequencer.sequence()` | TOCTOU 竞争防护 |
| 请求 ID 唯一 | `record_by_request.entry()` | 重复提交拒绝 |
| 操作 ID 幂等 | `seen_op_ids` | 防止双重结算 |
| 生命周期单向演进 | `is_valid_transition()` | 非法转换拒绝 |
| 价格-时间优先 | `match_incoming()` 订单簿遍历 | 撮合公平性 |
| 保证金充足 | `verify_sufficient_balance()` | 交易拒绝 |

---

## 崩溃恢复矩阵

| 崩溃时机 | 恢复后状态 | 数据一致性 | 需要人工干预 |
|----------|-----------|-----------|-------------|
| Sequencer WAL 写入后 | 记录存在，重放跳过 | ✅ 一致 | 否 |
| 撮合完成后、结算前 | 订单簿已变，ledger 未变 | ⚠️ 不一致 | 是（市场暂停） |
| Ledger WAL 写入后、内存更新前 | WAL 存在，重放重建 | ✅ 一致 | 否 |
| 清算拍卖进行中 | 从 WAL 重建拍卖状态 | ✅ 一致 | 否 |
| 资金费率转移中 | op_id 幂等保护 | ✅ 一致 | 否 |
| 触发单激活后崩溃 | 触发单已移除，普通订单已提交 | ✅ 一致 | 否 |
