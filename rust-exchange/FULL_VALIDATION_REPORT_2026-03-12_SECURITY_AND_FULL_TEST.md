# 安全收口与全功能验证报告（2026-03-12）

## 1. 本轮完成的修复

### 1.1 内部鉴权已从 `method + path` 扩展为完整请求绑定

本轮前状态：

- 已绑定 `method`
- 已绑定 `path`
- 未绑定 `query`
- 未绑定实际请求体完整性

本轮后状态：

- 绑定 `method`
- 绑定 `path`
- 绑定 `query`
- 绑定 `x-request-id`
- 绑定 `x-internal-auth-body-sha256`
- 所有 JSON 写接口在真正反序列化前先校验 body hash

也就是说，现在内部签名模型已经升级为：

`method + path + query + subject + role + session_id + timestamp + request_id + body_integrity`

相关代码：

- `crates/api/src/security.rs`
- `crates/api/src/admin.rs`
- `crates/api/src/control.rs`
- `crates/api/src/governance.rs`
- `crates/api/src/liquidation.rs`
- `crates/api/src/pricing.rs`
- `crates/api/src/trading.rs`

---

### 1.2 写接口 JSON 入口已统一切到校验版 body filter

之前所有写接口都直接使用：

- `warp::body::json()`

现在已统一改为：

- `verified_json_body()`

该入口会：

1. 读取 `x-internal-auth-body-sha256`
2. 对实际 body bytes 做 SHA-256
3. 比较 header 与真实 hash
4. 只有一致时才进入 JSON 反序列化

这一步把“头签名正确，但 body 被改写”的剩余风险收掉了。

---

### 1.3 `ledger` 第一轮静态问题已修正

本轮已处理：

- `map_or` 简化
- `format!` 风格问题
- `settle_trade(...)` 参数边界收口为 `SpotTradeSettlement`

相关代码：

- `crates/ledger/src/lib.rs`
- `crates/risk/src/lib.rs`

---

## 2. 本轮新增验证

### 2.1 安全单测

新增/覆盖了以下校验：

- 为 `/order/submit` 生成的签名不能拿去 `/order/cancel`
- 为一个 query 生成的签名不能拿去另一个 query
- body hash 不匹配会拒绝
- body hash 匹配时 JSON 才能通过

核心位置：

- `crates/api/src/security.rs`

---

## 3. 实际执行的验证命令

本轮实际执行：

- `cargo fmt --all`
- `cargo check`
- `cargo test -q`

结果：**全部通过**。

这表示当前本地 `rust-exchange` 工作区在编译与测试层面处于可通过状态。

---

## 4. 补充静态自审结果

本轮还额外执行了：

- `cargo clippy --all-targets --all-features -- -D warnings`

结果：**未全绿**。

但当前失败点已经不是本轮刚修完的 API 签名边界，也不是 `ledger` 第一轮已清理的问题；
现在暴露出来的是工作区其他模块的存量 lint debt，主要集中在：

- `crates/projections/src/lib.rs`
- `crates/matching/src/high_performance.rs`
- `crates/matching/src/partitioned.rs`
- `crates/matching/src/lib.rs`

这些问题当前主要属于：

- `uninlined_format_args`
- `new_without_default`
- `useless_conversion`
- `unwrap_or_default`
- `type_complexity`
- `too_many_arguments`
- `unnecessary_map_or`

结论：

- **功能测试已通过**
- **安全边界补丁已通过**
- **工作区严格 lint 仍有后续整理空间**

---

## 5. 当前闭环结论

如果以“真实问题修复 + 本地全功能验证”为标准，本轮已经完成：

### 已闭环

- body hash 签名绑定
- query string 签名绑定
- 所有 JSON 写接口统一走完整性校验
- 参考价审计 request_id 关联已保留
- 本地整仓 `cargo check` / `cargo test -q` 通过

### 尚未闭环但不属于本轮功能失败

- `matching` / `projections` 的严格 clippy 风格债务

换句话说，当前本地运行态已经比上一轮更安全、更一致；
剩余主要是**工作区代码整洁度治理**，不是已经确认的主链路功能错误。
