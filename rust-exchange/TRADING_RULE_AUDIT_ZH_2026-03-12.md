# Rust Exchange 交易规则逐条审计报告（2026-03-12）

## 1. 审计范围

本报告按“当前代码真实规则”逐条审查以下交易与风控规则：

- 鉴权与写请求完整性
- 用户身份绑定
- 下单基本校验
- `reduce_only`
- `replace`
- 自成交防护
- 价格带与市场状态
- 撤单与批量撤单
- 账本结算与 journal
- snapshot + replay 恢复
- 强平 / ADL / socialized loss

结论只基于当前 `rust-exchange` 本地代码，不基于外部设计口号。

---

## 2. 规则逐条审计

### 规则 1：所有写请求都必须经过认证与完整性校验

**当前状态：通过**

当前写接口已经统一走：

- `with_principal()`
- `verified_json_body()`

因此写请求需要同时满足：

- principal 签名有效
- role/subject 合法
- body hash 与真实 body 一致

审计结论：

- 当前不是匿名可写接口
- 当前不是“只校验头，不校验 body”的不完整签名模型

---

### 规则 2：用户动作不能信任 body 中伪造的 `user_id`

**当前状态：通过**

当前 API 层已有：

- `require_user(...)`
- `ensure_subject_matches(...)`
- `ensure_subject_or_admin(...)`

审计结论：

- 用户态写操作已经要求认证主体与目标用户一致
- 管理员路径则显式作为特权分支存在

---

### 规则 3：下单必须先 sequencing，再进入风险与撮合

**当前状态：通过**

当前 `sequence_new_order(...)` / `sequence_replace_order(...)` 先产生命令与 `command_seq`，
后续才进入风险校验和撮合执行。

审计结论：

- 命令顺序真相已经独立出来
- replay 有唯一命令源

---

### 规则 4：`reduce_only` 不能仅凭余额存在性放行

**当前状态：通过**

当前 `risk` 已实现：

- Spot 看可用持仓 + hold
- Margin / Perpetual 看衍生品净头寸
- 再减去已经保留给卖单的占用量

审计结论：

- 这条规则当前已经是实装，而不是待办

---

### 规则 5：`replace` 必须是原子可预测的

**当前状态：通过**

当前 `replace` 已修为：

- 先校验新单
- 失败则旧单保留
- 成功才做旧单撤销 + 新单进入
- 优先级丢失

审计结论：

- 当前已避免“先删旧单，再发现新单非法”的脏状态

---

### 规则 6：自成交必须被显式阻断

**当前状态：通过**

当前撮合层会返回：

- `SubmissionError::SelfTradePrevented(...)`

审计结论：

- 当前不是默认允许自成交
- 当前不是隐式吞单型副作用行为

---

### 规则 7：价格带与市场状态必须成为硬性入场门槛

**当前状态：通过**

当前撮合层存在：

- `PriceBandBreached`
- `MarketClosed`
- `KillSwitchActive`

审计结论：

- 价格带与市场状态已经是撮合前置门槛
- 管理态可把市场切到非正常态，普通命令会被拒绝

---

### 规则 8：撤单和批量撤单必须受身份边界约束

**当前状态：通过**

当前已经区分：

- 用户自己撤单 / mass cancel user/session
- 管理员 market 级 mass cancel / kill switch / market state

审计结论：

- 当前角色边界明确
- 不是“谁都可以调控制面”

---

### 规则 9：结算失败不能留下半提交状态

**当前状态：通过**

当前 crash recovery drill 的真实结果表明：

- 结算失败时市场进入 `Halted`
- 订单簿不会被错误清空
- journal 失败时不会留下脏 journal 记录

审计结论：

- 失败路径当前已具备“失败不脏状态”的保护

---

### 规则 10：恢复必须按 partition cursor 判断，不允许全局近似

**当前状态：通过**

当前恢复逻辑已经按每个 partition 的 `last_applied_command_seq` 决定是否 replay。

审计结论：

- 这是当前恢复正确性的核心边界
- 当前已避免快分区把慢分区命令跳过去的问题

---

### 规则 11：Spot 成交和衍生品成交必须区分结算语义

**当前状态：通过**

当前账本已明确区分：

- `settle_trade(SpotTradeSettlement)`
- `settle_derivative_trade(...)`

审计结论：

- 账本接口边界比之前更清晰
- Spot 与 Derivatives 已不是同一套弱语义参数拼装

---

### 规则 12：强平后续处理必须有 waterfall 路径

**当前状态：通过，但仍可继续增强**

当前强平执行后已经具备：

- penalty
- insurance path
- ADL ranking
- socialized loss

审计结论：

- 当前不是“只有强平，没有后续损失分配治理”
- 但更高阶运营治理与人工审批工作流还可以继续增强

---

## 3. 本轮大链路测试对应的规则验证

### 3.1 恢复演练

实际演练结果：

- 下单崩溃前：恢复后 `open_orders=1 state=Normal`
- 成交结算失败：`open_orders=1 state=Halted`
- journal 预写失败：`open_orders=1 state=Halted trade_entries=0`
- journal 后恢复：`open_orders=0 state=Normal trade_entries=1`

这验证了：

- 失败不脏状态
- 恢复边界正确
- journal 与订单簿不会进入半提交状态

### 3.2 延迟压测

实际结果：

- passive insert：`p50=78us p75=99us p99=247us`
- taker match：`p50=767us p75=1092us p99=1638us`

这说明当前单分区、无网络链路下，撮合内核路径已经具备较稳定的微秒/亚毫秒级行为。

### 3.3 吞吐压测

已执行 `1k` 与 `10k` 两档样本，在并发 `1 / 4 / 8` 下测试：

- passive path 吞吐最高约 `33.8k ops/s`
- taker match path 在 `10k` 样本下约 `0.76k ~ 0.80k ops/s`
- cancel path 在 `10k` 样本下约 `3.77k ~ 4.31k ops/s`

审计结论：

- 当前 passive/cancel 明显更轻
- taker match 成本显著更高，符合真实成交链路更重的设计预期

---

## 4. 当前仍属“可继续增强”的规则点

这些不是当前已确认 bug，而是更高阶能力建设点：

- 更细粒度 operator workflow
- 更复杂 liquidation auction / retry tiers
- 更细价格源治理与多源仲裁策略
- 更完整读模型服务化与指标观测

---

## 5. 审计总评

当前 `rust-exchange` 的交易规则已经不是“散落在代码里的隐式行为”，而是可以被逐条陈述并验证的显式规则集合。

一句话总结：

**当前交易规则、风控规则、恢复规则和失败规则已经基本形成可审计的正式主线。**
