# 本地安全、稳定性与压力验证报告

日期：2026-03-23  
仓库：`d:\pre_trading`

## 1. 结论摘要

本轮已经完成一轮“本地可真实执行”的安全、稳定性与压力验证，并同步落地了关键加固代码。

- 已完成：后端关键写接口的请求体签名绑定加固
- 已完成：安全负向回归测试补充并实跑
- 已完成：Rust API 全量测试、Chaos/Stress Suite、Release 压测样例
- 已完成：Go 主线测试、前端 benchmark guard
- 未完成：第三方正式外部安全审计
- 未完成：真实公网黑盒渗透测试
- 未完成：多天级别线上 soak 验证

当前可给出的真实结论是：

- 本地代码层面的认证完整性明显增强，关键写接口已经不再接受“签名身份但未签名 body”的请求
- Rust 交易核心在本地 chaos/stress 与 release bench 下表现稳定，账本与分区健康检查均通过
- 前端和 Go 主线当前构建/测试/benchmark guard 通过
- 仍然不能把本轮结果等同于“已完成外部审计”或“已完成真实线上长期稳定性验证”

## 2. 本轮实际执行范围

| 目标 | 本地是否可真实执行 | 本轮执行情况 | 结论 |
|---|---|---|---|
| 正式外部安全审计 | 否 | 未执行 | 需第三方审计团队/独立机构 |
| 严格渗透测试 | 部分可执行 | 已做本地负向测试与认证回归 | 可视为本地安全验证，不等于黑盒渗透 |
| 长期线上稳定性验证 | 部分可执行 | 已做本地 stress/chaos 验证 | 不等于 7x24 线上 soak |
| 大规模真实压力 | 可部分执行 | 已做 release bench 与 stress suite | 属于本地高负载验证，不等于生产流量 |

## 3. 本轮代码加固

本轮已将以下关键写接口切换到 `verified_json_body()`，要求请求体提供并匹配 `X-Internal-Auth-Body-Sha256`：

- `transfer`
- `withdraw`
- `admin/withdrawal/approve`
- `admin/withdrawal/reject`
- `whitelist/address`
- `admin/whitelist/address`
- `withdraw/dry-run`
- `admin/fee-tiers`
- `otc/quotes`
- `earn/subscribe`
- `earn/redeem`
- `admin/sentinel/override`
- `admin/sentinel/resolve-origin`

这次加固覆盖的核心风险是：

- 身份头已签名，但请求体内容未参与摘要校验
- 中间层或代理误改 body 后，后端仍可能接受
- 资金、提现、白名单、管理类操作的请求完整性不足

## 4. 新增测试

本轮新增了路由级安全回归测试，重点验证“缺失 body hash 的真实请求会被拒绝”：

- `security::tests::transfer_route_rejects_missing_body_hash`
- `security::tests::transfer_route_accepts_matching_body_hash`
- `security::tests::sentinel_override_route_rejects_missing_body_hash`

这些测试不是纯函数单测，而是带真实 Warp 路由过滤器与认证头的负向/正向回归。

## 5. 实际执行命令

### Rust 核心

```powershell
cargo test -p api -- --nocapture
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p matching --example latency_bench --release
$env:SCALE_BENCH_SAMPLES='25000'; cargo run -p matching --example scale_bench --release
```

### Go 主线

```powershell
go test ./...
```

### 前端

```powershell
npm run benchmark:check
```

## 6. 实测结果

### 6.1 Rust API 测试

- `cargo test -p api -- --nocapture`：通过
- 结果：`223 passed, 0 failed`
- 附带执行了内置 `stress_full_suite_runs`

### 6.2 Rust 质量门

- `cargo clippy --workspace --all-targets -- -D warnings`：通过

### 6.3 Rust Chaos / Stress Suite

内置 8 个场景全部通过：

| 场景 | 类型 | 结果 |
|---|---|---|
| Queue Saturation | Extreme | PASS |
| Burst Spike | Extreme | PASS |
| WAL Storm | Extreme | PASS |
| Settlement Cascade | Extreme | PASS |
| Kill Switch Storm | Chaos | PASS |
| Concurrent Cancel | Chaos | PASS |
| Backpressure Ramp | Extreme | PASS |
| Snapshot Recovery | Recovery | PASS |

最终一次实跑的关键指标：

| 场景 | 持续时间 | 吞吐 |
|---|---|---|
| Queue Saturation | 114ms | 13,158 ops/sec |
| Burst Spike | 96ms | 6,849 ops/sec |
| WAL Storm | 135ms | 7,407 ops/sec |
| Settlement Cascade | 73ms | 4,110 ops/sec |
| Backpressure Ramp | 55ms | 14,545 ops/sec |

一致性结果：

- `ledger_balanced = true`
- `partitions_healthy = true`
- `kill_switch_off = true`

### 6.4 Matching Release Latency Bench

`cargo run -p matching --example latency_bench --release`

| workload | samples | p50 | p75 | p99 | avg | max | throughput |
|---|---:|---:|---:|---:|---:|---:|---:|
| passive_limit_insert | 5000 | 18us | 36us | 156us | 30.48us | 1824us | 32,251 ops/sec |
| taker_limit_match | 5000 | 174us | 262us | 492us | 190.35us | 2268us | 5,238 ops/sec |

### 6.5 Matching Release Scale Bench

`cargo run -p matching --example scale_bench --release`

测试参数：

- `SCALE_BENCH_SAMPLES=25000`
- 并发级别：`1 / 4 / 8 / 16`

关键结果摘要：

| 模式 | 并发 | p50 | p99 | avg | throughput |
|---|---:|---:|---:|---:|---:|
| passive | 1 | 35us | 155us | 47.20us | 20,935 ops/sec |
| passive | 16 | 1083us | 2773us | 1181.78us | 13,517 ops/sec |
| taker | 1 | 1139us | 5569us | 1433.29us | 697 ops/sec |
| taker | 16 | 23482us | 84218us | 27032.65us | 592 ops/sec |
| cancel | 1 | 145us | 353us | 143.94us | 6,921 ops/sec |
| cancel | 16 | 1773us | 4114us | 1916.77us | 8,342 ops/sec |

解释：

- 被动挂单路径扩展性较好，但高并发下延迟明显上升
- taker 撮合路径在并发上升后尾延迟放大最明显，是当前最值得继续优化的热点
- cancel 路径整体较稳，吞吐与延迟都优于 taker match

### 6.6 Go 主线

- `go test ./...`：通过
- 通过模块包括：`benchmark`、`ledger`、`matching`、`risk`、`simulator`

### 6.7 前端 Benchmark Guard

- `npm run benchmark:check`：通过

关键结果：

| case | ops/sec |
|---|---:|
| safeDivide hot path | 57,527,469 |
| safeWeightedAverage small vector | 1,877,174 |
| classifyChange state + grade + value | 6,866,344 |
| StableList.update (batch=128) | 8,020 |
| SignalHysteresis.addSample | 775,241 |
| formatPercentChange | 4,789,536 |

## 7. 本轮可以下结论的安全判断

### 已验证并修复

- 关键写接口的 body 完整性校验已经落地
- 缺失 `X-Internal-Auth-Body-Sha256` 的关键写请求已被路由级测试验证会拒绝
- 匹配 body hash 的合法转账请求已被路由级测试验证可通过

### 已验证但仍需继续加强

- Rust 内部认证、时间戳、请求 ID、重放保护总体可用
- 治理审批主线已比之前更完整，但仍有部分高影响管理员动作没有完全收口到治理队列
- release bench 表明核心撮合可承压，但 taker 路径高并发尾延迟仍偏高

## 8. 本轮不能夸大成已完成的事项

以下事项本轮没有完成，报告中不能写成“已通过”：

- 第三方正式外部安全审计
- 公网黑盒渗透测试
- 红队演练
- 多机部署环境下的真实网络抖动与故障注入
- 多天级别线上 soak test
- 真实生产用户流量压测

## 9. 下一步建议

优先级 P0：

- 将剩余高影响管理员写操作继续收口到 governance queue
- 为更多资金/治理路由补充“缺 hash 拒绝、错 hash 拒绝、正确 hash 通过”的路由级测试

优先级 P1：

- 对 taker 撮合路径做火焰图与热点分析
- 增加 release 模式下的固定压测脚本与阈值门禁
- 增加 Prometheus 指标的压力回归基线

优先级 P2：

- 引入第三方外部审计
- 在独立环境做长时间 soak test
- 做一次带代理、重放、篡改、中间人假设的黑盒渗透测试

## 10. 最终状态

截至本报告生成时：

- Rust 安全回归：通过
- Rust 稳定性/压力验证：通过
- Rust `clippy` 质量门：通过
- Go 主线测试：通过
- 前端 benchmark guard：通过

本轮交付可以定义为：

“已完成一轮可信的本地安全加固与压力验证，并形成了可复现的测试结果；但尚未替代正式外部审计与线上长期稳定性验证。”
