# Rust Exchange 大链路测试报告（2026-03-12）

## 1. 测试目标

本轮测试目标不是网络/网关压测，而是直接验证 Rust 主线核心：

- 恢复链路
- 失败路径一致性
- passive insert 延迟
- taker match 延迟
- 1k / 10k 样本吞吐表现

测试入口全部来自现有真实代码：

- `crates/matching/examples/crash_recovery_drill.rs`
- `crates/matching/examples/latency_bench.rs`
- `crates/matching/examples/scale_bench.rs`

---

## 2. 实际执行命令

### 2.1 恢复演练

`cargo run -q -p matching --example crash_recovery_drill`

### 2.2 延迟基准

`cargo run -q -p matching --example latency_bench`

### 2.3 吞吐压测（1k 样本）

`SCALE_BENCH_SAMPLES=1000`

`SCALE_BENCH_LEVELS=1,4,8`

`cargo run -q -p matching --example scale_bench`

### 2.4 吞吐压测（10k 样本）

`SCALE_BENCH_SAMPLES=10000`

`SCALE_BENCH_LEVELS=1,4,8`

`cargo run -q -p matching --example scale_bench`

---

## 3. 恢复演练结果

实际输出：

- `scenario=order_crash_before_partition_apply result=ok open_orders=1 state=Normal`
- `scenario=settlement_crash_during_match result=ok error=Ledger("forced ledger wal failure for settle_trade:req-req-settle-2:0:0") open_orders=1 state=Halted`
- `scenario=journal_pre_append_failure result=ok error=Persistence("forced trade journal failure") open_orders=1 state=Halted trade_entries=0`
- `scenario=journal_post_append_recovery result=ok open_orders=0 state=Normal trade_entries=1`

结论：

- 下单中断后可恢复到单一真相
- 结算失败会进入 `Halted`，且不会脏写订单簿
- journal append 失败不会留下虚假 trade entry
- journal 后恢复路径能正确去重与收敛

---

## 4. 延迟基准结果

实际输出：

### 4.1 Passive limit insert

- samples=`5000`
- p50=`78us`
- p75=`99us`
- p99=`247us`
- avg=`85.34us`
- max=`2168us`
- throughput=`11644.74 ops/s`

### 4.2 Taker limit match

- samples=`5000`
- p50=`767us`
- p75=`1092us`
- p99=`1638us`
- avg=`774.15us`
- max=`3954us`
- throughput=`1290.45 ops/s`

结论：

- passive insert 已经处于较低延迟区间
- taker match 明显更重，符合真实成交结算链路成本更高的预期

---

## 5. 吞吐压测结果（1k 样本）

### 5.1 concurrency=1

- passive：p50=`27us` p75=`31us` p99=`67us` throughput=`33838.08 ops/s`
- taker：p50=`171us` p75=`238us` p99=`375us` throughput=`5714.70 ops/s`
- cancel：p50=`49us` p75=`58us` p99=`76us` throughput=`20057.08 ops/s`

### 5.2 concurrency=4

- passive：p50=`117us` p75=`138us` p99=`386us` throughput=`30992.86 ops/s`
- taker：p50=`737us` p75=`1027us` p99=`1358us` throughput=`5340.52 ops/s`
- cancel：p50=`196us` p75=`237us` p99=`473us` throughput=`19112.27 ops/s`

### 5.3 concurrency=8

- passive：p50=`262us` p75=`308us` p99=`606us` throughput=`29118.02 ops/s`
- taker：p50=`1508us` p75=`2114us` p99=`2989us` throughput=`5221.15 ops/s`
- cancel：p50=`381us` p75=`438us` p99=`823us` throughput=`20449.48 ops/s`

结论：

- 1k 样本下，passive 和 cancel 路径都有很高的吞吐空间
- taker 路径受撮合+结算链路影响，吞吐显著更低但仍稳定

---

## 6. 吞吐压测结果（10k 样本）

### 6.1 concurrency=1

- passive：p50=`98us` p75=`139us` p99=`221us` throughput=`9702.80 ops/s`
- taker：p50=`1277us` p75=`1841us` p99=`2573us` throughput=`787.23 ops/s`
- cancel：p50=`256us` p75=`364us` p99=`524us` throughput=`3768.44 ops/s`

### 6.2 concurrency=4

- passive：p50=`438us` p75=`597us` p99=`889us` throughput=`9308.19 ops/s`
- taker：p50=`4941us` p75=`7545us` p99=`9869us` throughput=`804.71 ops/s`
- cancel：p50=`929us` p75=`1288us` p99=`1696us` throughput=`4215.77 ops/s`

### 6.3 concurrency=8

- passive：p50=`906us` p75=`1180us` p99=`1738us` throughput=`8959.82 ops/s`
- taker：p50=`10316us` p75=`15010us` p99=`23564us` throughput=`762.42 ops/s`
- cancel：p50=`1769us` p75=`2617us` p99=`3448us` throughput=`4312.47 ops/s`

结论：

- 10k 样本下系统保持稳定，没有出现失控型长尾或错误爆发
- passive 路径吞吐在 `~9k ops/s`
- cancel 路径吞吐在 `~3.8k ~ 4.3k ops/s`
- taker 真成交路径在 `~0.76k ~ 0.80k ops/s`

---

## 7. 测试结论

如果把本轮目标定义为“验证 Rust 主线在恢复、失败路径、撮合延迟、吞吐上的基本闭环”，当前结论是：

- 恢复链路：通过
- 失败路径一致性：通过
- passive/cancel：稳定
- taker match：稳定但明显更重
- 1k / 10k 样本：均可完成，无异常退出

一句话总结：

**当前 Rust 主线已经能够在真实恢复演练和中等规模链路压测下保持正确与稳定。**
