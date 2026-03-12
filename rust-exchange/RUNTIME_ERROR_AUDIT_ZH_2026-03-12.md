# Rust Exchange 运行时错误与一致性审计报告（2026-03-12）

## 1. 本轮已经确认并修复的内容

### 1.1 内部鉴权签名未绑定路由路径

之前内部鉴权签名载荷只绑定：

- HTTP method
- subject
- role
- session_id
- timestamp
- request_id

这意味着：

- 同一个签名在同一 method 下
- 只要还在时间窗口内
- 理论上可被拿去尝试访问另一个同 method 的路由

这是一个**真实的鉴权绑定面缺口**。

本轮已修复：

- 新的签名载荷已额外绑定 `path`
- `with_principal()` / `with_optional_principal()` 会把实际请求路径送入校验
- 已补单测，验证“为 `/order/submit` 签的名不能拿去 `/order/cancel` 用”

相关实现：

- `crates/api/src/security.rs`

---

### 1.2 手工参考价更新的审计 `request_id` 之前是写死值

之前 `reference-price` 管理接口的审计记录使用固定字符串：

- `manual-reference`

这会带来两个问题：

- 无法与具体请求实例关联
- 审计与排障时无法跨日志做唯一追踪

本轮已修复：

- `ReferencePriceRequest` 增加 `request_id`
- 路由中对 `request_id` 做标准化
- 审计日志使用真实 `request_id`
- HTTP 返回中也回显 `request_id`

相关实现：

- `crates/api/src/dto.rs`
- `crates/api/src/control.rs`

---

### 1.3 `main.rs` 剩余 shared helpers 已继续收口

本轮把原先仍留在 `main.rs` 的 shared helpers 继续拆分：

- `security.rs`
  - internal auth
  - principal filters
  - role/subject guard
- `helpers.rs`
  - request_id 归一化
  - client_order_id 归一化
  - audit helper
  - lifecycle marker helper

这不是直接业务修复，但它显著降低了“入口文件继续膨胀、审计边界模糊”的结构风险。

---

## 2. 本轮自审后仍确认存在的问题

以下问题是**仍然存在**、但本轮没有继续往下做的：

### 2.1 内部鉴权仍未绑定请求体哈希

虽然现在签名已经绑定 `method + path + principal + timestamp + request_id`，
但**尚未绑定 body hash**。

因此对于同一路径下的写请求，理论上仍存在这类剩余风险：

- 如果攻击者能复用同一组头
- 且还能改写请求体
- 在时间窗口内仍可能构造“同 path 不同 body”的请求尝试

这不是当前代码已经被利用的证明，但从安全建模上，它仍然是一个**真实剩余缺口**。

建议下一步：

- 增加 `x-internal-auth-body-sha256`
- 将 body hash 纳入 HMAC 载荷
- 对空 body 使用固定空串 hash

---

### 2.2 查询参数未纳入内部鉴权签名

当前签名绑定了 path，但没有显式绑定 query string。

对于多数核心写接口，这个问题影响较小，因为它们主要依赖 JSON body；
但如果后续出现基于 query 参数驱动语义的敏感路由，这会变成新的绑定缺口。

建议下一步：

- 若存在 query 参与鉴权语义的接口
- 将 canonical query string 一并纳入签名

---

### 2.3 `clippy -D warnings` 当前未全绿

本轮实际执行了：

- `cargo clippy -p api -- -D warnings`

结果：**失败**。

但失败并不是因为本轮新增的 `api` 结构收口，而是工作区其他 crate 里已有静态问题。最集中的是：

- `crates/ledger/src/lib.rs`

当前能确认的类别包括：

- `unnecessary_map_or`
- `uninlined_format_args`
- `too_many_arguments`

其中唯一带“接口设计味道”的项是：

- `crates/ledger/src/lib.rs:494` 的 `settle_trade(...)` 参数过多

这更像维护性问题，不是已确认的运行时错误；
但它说明**代码质量门槛还没有完全收拢到严格静态检查级别**。

---

## 3. 本轮没有发现的错误

本轮编译与测试范围内，没有发现以下内容被本次改动打坏：

- API 编译
- 路由拆分后的调用边界
- bootstrap/runtime 启动链路
- snapshot + partition-aware replay 收口
- liquidation / funding scheduler 启动路径

换句话说，本轮属于：

- 修正一个真实审计关联错误
- 修正一个真实鉴权绑定面缺口
- 再做一轮结构收口

而不是引入新的交易语义变化。

---

## 4. 本轮实际验证

本轮已执行并通过：

- `cargo fmt --all`
- `cargo check -p api`
- `cargo test -q`

本轮已执行但未通过：

- `cargo clippy -p api -- -D warnings`

失败原因如上，主要来自 `ledger` 现有静态问题，不是本轮 `api` 收口本身。

---

## 5. 审计结论

截至当前，可以比较明确地说：

### 已经修好的

- 内部鉴权不再只是“method 级绑定”，而是“method + path”绑定
- 手工参考价更新已经具备真实 `request_id` 审计关联
- API 入口 shared helpers 进一步模块化，`main.rs` 更接近干净入口

### 仍需继续补的

- body hash 绑定
- query string 绑定策略
- `ledger` 的 clippy/接口整洁度收口

### 当前判断

本轮之后，`rust-exchange` 的 API 入口已经比上一轮更可控、更容易审计；
但如果目标是交易所级内部控制面签名模型，下一步必须继续把**请求体完整性**也纳入签名边界。
