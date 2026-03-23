# Rust Exchange 架构运行对齐说明

> 生成日期：2026-03-23
> 适用范围：`d:\pre_trading\rust-exchange`
> 文档定位：本文件用于替代今天那份已出现乱码的 `ARCHITECTURE_ZH.md` 运行时口径说明，重点描述“代码当前真实做到了什么”。

## 本次对齐目标

本轮聚焦四件事：

- 把提现治理链从“入口直接拒绝”改成可执行、可审计的审批流。
- 把提现额度、地址额度和 vault velocity 的记账时机改到最终批准执行时。
- 把提现资产语义收敛成当前真实支持的 `USDC` 单资产版本。
- 把 Rust 工作区恢复到 `fmt/check/test/clippy` 全绿。

## 当前主链路

`API -> Sequencer/WAL -> Risk -> Matching -> Ledger -> Projection/WebSocket`

这条交易主链路没有在本轮被改写；本次修复集中在提现与工程门禁。

## 提现运行时口径

当前提现生命周期以 `pending` 为中心，终态包括：

- `approved`
- `rejected`
- `cancelled`
- `expired`

当前实现要点：

- 入口仍会检查白名单、用户额度、地址额度、sentinel posture、breaker 和 vault velocity。
- 校验通过后先创建账本 hold，并写入一条 `pending` 提现记录。
- 提现记录现在持久化 `required_approvals` 与 `approvers`。
- 审批不足时继续保持 `pending`，并返回剩余审批数。
- 达到审批阈值后，才执行签名校验、vault 划转以及最终记账。

## 本次已落地修复

### 1. 提现治理链修复

- `RequiresGovernance` 不再在入口直接 `403`。
- 治理升级提现现在能进入待审批记录。
- 同一管理员不能重复审批同一笔提现。
- 审批动作按 `required_approvals` 累积，而不是单管理员直接放行。

### 2. 额度与速度记账时机修复

以下统计改为在最终批准执行时落账：

- `WithdrawalUsageTracker`
- `AddressUsageTracker`
- `WithdrawalVelocityTracker`

这样取消、拒绝、过期的提现不会错误占用额度，也不会误触发 velocity breaker。

### 3. 提现资产语义收敛

- 提现资产在入口会规范化。
- 当前运行时只允许 `USDC`。
- 非 `USDC` 请求会被拒绝，避免接口语义和账本执行语义分叉。

### 4. 工程门禁修复

本轮已清理导致 Rust CI 失败的 `clippy` 问题，包括：

- `manual_flatten`
- 可派生 `Default`
- benchmark `unit_arg`
- `manual_range_contains`
- `field_reassign_with_default`
- `uninlined_format_args`
- `single_match`
- `useless_vec`

## 已验证结果

以下命令已在本地通过：

```powershell
cargo fmt --all --check
cargo check --workspace --all-targets
cargo test -q
cargo clippy --workspace --all-targets -- -D warnings
```

## 当前仍保留的真实约束

### Hot 档当前不是自动出金

当前运行时虽然不再有 time-lock，但仍统一走：

`pending -> admin approve -> execute`

如果后续目标是恢复真正的 `Hot = 0 approvals` 自动执行，还需要单独设计直出路径。

### 提现资产仍是单资产

如果后续要支持多资产提现，还需要同步扩展：

- 余额检查
- hold 账户模型
- vault 路由
- 签名层
- 地址格式校验
- 风险与额度统计维度

在这些没有同时完成之前，`USDC only` 是当前最真实、最安全的产品口径。
