# 快速开始指南 (Quick Start Guide)

## 系统概述

这是一个预测市场平台，实现了以下核心功能：

### ✅ 已实现的功能

1. **FBA 批量撮合引擎** - 每 500ms 执行一次批量撮合
2. **双记账本系统** - 严格的借贷平衡验证
3. **链上事件索引** - 监听存款/提款并处理重组
4. **风控管理** - 市场状态机 + 多级熔断机制
5. **实时 API** - REST + WebSocket 支持
6. **事件驱动架构** - 服务间通过事件总线通信

## 快速启动

### 1. 安装依赖

```powershell
# 确保已安装 Go 1.21+
go version

# 下载依赖
go mod tidy
```

### 2. 启动所有服务

```powershell
# 使用启动脚本（推荐）
.\start.ps1

# 或手动启动每个服务
cd ledger && go run main.go
cd matching && go run main.go
cd indexer && go run main.go
cd risk && go run main.go
cd api && go run main.go
```

### 3. 测试 API

```powershell
# 运行测试脚本
.\test_api.ps1

# 或手动测试
Invoke-RestMethod -Uri "http://localhost:8080/health"
```

## 服务端口

- **API Gateway**: 8080 (REST + WebSocket)
- **Ledger Service**: 8081
- **Matching Engine**: 8082
- **Indexer Service**: 8083
- **Risk Service**: 8084

## 核心概念

### 账本账户格式

```
用户账户:
- U:user_id:USDC              # 可用余额
- U:user_id:USDC:HOLD         # 冻结余额
- U:user_id:OUTCOME:market:N  # 结果份额

市场账户:
- M:market_id:ESCROW:USDC     # 托管资金
- M:market_id:FEE:USDC        # 手续费
- M:market_id:OUTCOME_POOL    # 结果池

系统账户:
- SYS:ONCHAIN_VAULT:USDC      # 链上金库
```

### 市场状态

1. PROPOSED - 市场提议
2. OPEN - 正常交易
3. CLOSE_ONLY - 只能平仓
4. CLOSED - 已关闭
5. FINALIZED - 已结算

### 熔断级别

- L1: 停止新仓位
- L2: 停止提款
- L3: 停止链上交易
- L4: 只读模式

## API 示例

### 创建订单意向

```powershell
$body = @{
    user_id = "user1"
    market_id = "market1"
    side = "buy"
    price = 55
    amount = 1000
    outcome = 0
    expires_in = 60
} | ConvertTo-Json

Invoke-RestMethod -Uri "http://localhost:8080/v1/intents" `
    -Method Post -Body $body -ContentType "application/json"
```

### 查询市场

```powershell
# 获取所有市场
Invoke-RestMethod -Uri "http://localhost:8080/v1/markets"

# 获取订单簿
Invoke-RestMethod -Uri "http://localhost:8080/v1/markets/market1/book"
```

### 查询余额

```powershell
Invoke-RestMethod -Uri "http://localhost:8080/v1/balances?user_id=user1"
```

### 取消订单

```powershell
Invoke-RestMethod -Uri "http://localhost:8080/v1/orders/{intent_id}/cancel" -Method Post
```

### 提款请求

```powershell
$body = @{
    user_id = "user1"
    amount = 1000
    address = "0x..."
} | ConvertTo-Json

Invoke-RestMethod -Uri "http://localhost:8080/v1/withdrawals" `
    -Method Post -Body $body -ContentType "application/json"
```

## WebSocket 连接

```javascript
const ws = new WebSocket('ws://localhost:8080/ws');

ws.onopen = () => {
    console.log('Connected to prediction platform');
};

ws.onmessage = (event) => {
    const data = JSON.parse(event.data);
    console.log('Event:', data.Type, data.Payload);
};
```

## 日志和监控

```powershell
# 查看日志
Get-Content .\logs\API.log -Tail 20
Get-Content .\logs\Matching.log -Tail 20

# 健康检查
Invoke-RestMethod -Uri "http://localhost:8080/health"
```

## 目录结构

```
pre_trading/
├── api/                    # API 网关
│   └── main.go
├── ledger/                 # 账本服务
│   └── main.go
├── matching/               # 撮合引擎
│   └── main.go
├── indexer/                # 索引服务
│   └── main.go
├── risk/                   # 风控服务
│   └── main.go
├── services/               # 共享代码
│   ├── types/             # 数据类型
│   ├── eventbus/          # 事件总线
│   └── utils/             # 工具函数
├── config.yaml            # 配置文件
├── start.ps1              # 启动脚本
├── test_api.ps1           # 测试脚本
└── README.md              # 详细文档
```

## 工作流程

### 订单撮合流程

1. 用户通过 API 提交意向 (Intent)
2. 意向进入撮合引擎的批次队列
3. 每 500ms 执行一次批量撮合:
   - 过滤有效意向
   - 计算清算价格
   - 按比例分配成交
4. 成交后生成账本增量 (Delta)
5. 账本服务验证并提交
6. 通过事件总线广播结果

### 存款流程

1. 用户在链上发起存款
2. Indexer 监听事件
3. 等待 6 个确认
4. 提交到账本服务
5. 更新用户余额
6. 发送确认事件

## 下一步

### 开发任务

1. [ ] 添加用户认证 (JWT)
2. [ ] 实现数据库持久化
3. [ ] 添加 Prometheus 指标
4. [ ] 实现订单簿快照
5. [ ] 添加单元测试
6. [ ] 实现市场创建 API
7. [ ] 添加结算流程
8. [ ] 实现费用计算

### 生产部署

1. [ ] 配置 PostgreSQL
2. [ ] 部署 Redis 缓存
3. [ ] 配置 Redpanda/Kafka
4. [ ] 设置 TLS 证书
5. [ ] 配置监控告警
6. [ ] 实现备份恢复
7. [ ] 安全审计
8. [ ] 压力测试

## 技术栈

- **语言**: Go 1.21+
- **路由**: Gorilla Mux
- **WebSocket**: Gorilla WebSocket
- **事件总线**: 内存实现 (生产用 Redpanda)
- **数据库**: 设计支持 PostgreSQL
- **缓存**: 设计支持 Redis

## 设计文档

详细技术设计请参考: `prediction_platform_full_design_v1_2.txt`

包含:
- 完整的服务架构
- 账本模板
- 撮合算法
- 重组处理
- 安全机制
- 运维手册

## 常见问题

### Q: 如何修改批次窗口?
A: 编辑 `config.yaml` 中的 `matching.batch_window_ms`

### Q: 如何增加确认数?
A: 编辑 `config.yaml` 中的 `indexer.confirmations`

### Q: 如何激活熔断机制?
A: 调用 Risk Service 的 `ActivateKillSwitch(level, reason)` 方法

### Q: 日志在哪里?
A: 所有日志在 `./logs/` 目录下，按服务名分文件

## 贡献

欢迎提交 Issue 和 Pull Request！

## 许可证

[待定]
