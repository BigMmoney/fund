# API DTO 拆分与边界审计报告（2026-03-12）

## 1. 本轮完成内容

本轮继续做了两件事：

1. 把 `crates/api/src/main.rs` 顶部集中定义的 `Request / Query / DTO` 全部抽到独立模块。
2. 再次审计当前 API 结构，判断是否还存在明显的结构性耦合或逻辑边界问题。

---

## 2. 已完成的结构拆分

### 2.1 新增 DTO 模块

新增：

- `crates/api/src/dto.rs`

目前该模块承载：

- 交易写接口 DTO
- 控制面 DTO
- 账户读接口 Query
- 市场读接口 Query
- 清算 / 治理 / 价格 / 资金费率相关 DTO

这意味着：

- `main.rs` 不再承担顶层 DTO 大列表
- 路由模块和类型模块已经开始分离
- 子模块现在通过统一 DTO 模块共享入参结构，而不是继续依赖 `main.rs` 顶部定义

### 2.2 当前 API 路由模块边界

现在 `crates/api/src/` 的主要结构是：

- `trading.rs` — 用户交易写路径
- `control.rs` — 管理员交易控制路径
- `accounts.rs` — 用户账户读接口
- `markets.rs` — 市场读接口与统计接口
- `admin.rs` — admin control plane 与 funding/risk control
- `pricing.rs` — 多源指数与 fair-price 路由
- `governance.rs` — 审批流与治理动作
- `liquidation.rs` — 清算队列、拍卖、保险金、worker
- `dto.rs` — API 入参/查询类型
- `main.rs` — bootstrap + route composition + scheduler startup

这已经比最初的单一 `main.rs` 实现清晰很多。

---

## 3. 这轮拆分后的真实状态

### 3.1 `main.rs` 还剩什么

现在 `main.rs` 主要保留：

- store / engine / registry 构建
- startup replay / recovery bootstrap
- rate limiter 初始化
- 各 route builder 的顶层拼接
- CORS / recover / bind
- automation scheduler 启动
- 若干通用 helper / store 定义

也就是说，`main.rs` 已经基本进入“应用装配器”角色。

### 3.2 当前还没有完全拆掉的部分

虽然路由和 DTO 已经抽出，但下面这些仍然集中在 `main.rs`：

1. `build_*_store()` 一组初始化函数
2. 一批 shared helper / normalization / lifecycle helper
3. 若干 store record / runtime record 类型
4. bootstrap/recovery 编排逻辑

这不是 bug，但说明还有下一层可以继续做：

- `bootstrap.rs` / `app_context.rs`
- `stores.rs`
- `helpers.rs` / `request_normalization.rs`

---

## 4. 审计结论

### 4.1 正确性层面

当前没有看到因为本轮拆分引入的新逻辑错误：

- 路由行为未改变，只是移动到独立模块
- DTO 未改变字段语义，只是集中到 `dto.rs`
- 编译通过、测试通过，说明当前重构没有破坏已有行为边界

### 4.2 并发与共享状态层面

当前也没有看到新的共享状态问题：

- 没有额外增加共享可变状态
- 没有改变 `Arc / DashMap / Mutex / WAL append` 关系
- 没有把撮合热路径错误地下沉到读接口模块或控制面模块中

### 4.3 结构层面

现在 API 层最大的剩余问题已经进一步收窄到：

- `main.rs` 仍然兼任 bootstrap 与 store wiring
- shared helper 还没有模块化
- DTO 虽已独立，但尚未细分为 `dto/trading.rs`、`dto/liquidation.rs` 这种二级结构

换句话说：

**当前剩余问题已经是“进一步工程化整理”，而不是大的架构方向问题。**

---

## 5. 推荐下一步

如果继续往下做，建议顺序如下：

### Step 1: 抽 `bootstrap.rs`

把以下逻辑从 `main.rs` 移走：

- engine/store/context 初始化
- snapshot restore + replay
- scheduler wiring

### Step 2: 抽 `stores.rs`

把各类 `Persistent*Store` / `build_*_store()` 迁移到更明确的初始化层。

### Step 3: DTO 二级分目录

把 `dto.rs` 继续拆成：

- `dto/trading.rs`
- `dto/control.rs`
- `dto/accounts.rs`
- `dto/markets.rs`
- `dto/risk.rs`

### Step 4: helper 模块化

把 request normalization、principal guards、audit helper、rejection helper 独立成 util/helper 模块。

---

## 6. 本轮验证

实际执行：

- `cargo fmt --all`
- `cargo check -p api`
- `cargo test -q`

结果：全部通过。

---

## 7. 一句话总结

本轮完成后，`rust-exchange` 的 API 层已经从“单文件巨石”进一步推进到：

**按路由域拆分、按 DTO 统一收口、主文件以 bootstrap/compose 为主的模块化结构。**

这说明当前架构已经不只是“能跑”，而是开始具备长期维护和继续下沉拆分的基础。
