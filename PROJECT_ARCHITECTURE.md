# 🏗️ Fund Management API - 项目架构文档

**版本**: v1.0  
**日期**: 2025-10-17  
**状态**: 生产环境运行中

---

## 📋 目录

- [1. 项目概述](#1-项目概述)
- [2. 技术架构](#2-技术架构)
- [3. 项目结构](#3-项目结构)
- [4. 数据库设计](#4-数据库设计)
- [5. 核心业务逻辑](#5-核心业务逻辑)
- [6. AWS 集成](#6-aws-集成)
- [7. 外部 API 集成](#7-外部-api-集成)
- [8. 安全机制](#8-安全机制)

---

## 1. 项目概述

### 1.1 项目定位

**Fund Management API** 是一个基金管理系统后端服务，主要功能包括：

- ✅ 用户认证与权限管理
- ✅ 投资组合（Portfolio）管理
- ✅ 收益分配与结算
- ✅ 提现与调账管理
- ✅ 数据快照与统计
- ✅ 与 OneToken、Ceffu 等外部服务集成

### 1.2 核心特性

| 特性 | 说明 |
|------|------|
| **RESTful API** | 标准 REST API 设计 |
| **JWT 认证** | 基于 Token 的无状态认证 |
| **权限控制** | 细粒度的权限管理 |
| **数据快照** | 按小时记录收益快照 |
| **多租户** | 支持团队（Team）隔离 |
| **外部集成** | OneToken + Ceffu API |

---

## 2. 技术架构

### 2.1 整体架构图

```
┌─────────────────────────────────────────────────────────────┐
│                        客户端层                              │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐   │
│  │ Web 前端 │  │ 移动端   │  │ 第三方   │  │ 内部工具 │   │
│  └──────────┘  └──────────┘  └──────────┘  └──────────┘   │
└────────────────────────┬────────────────────────────────────┘
                         │ HTTP/HTTPS
                         ▼
┌─────────────────────────────────────────────────────────────┐
│                      接入层（Nginx）                         │
│  ┌───────────────────────────────────────────────────────┐  │
│  │  - 反向代理（Port 80 → 8000/8001）                    │  │
│  │  - 负载均衡                                           │  │
│  │  - SSL 终止（未来）                                   │  │
│  └───────────────────────────────────────────────────────┘  │
└────────────────────────┬────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────────┐
│                  应用层（FastAPI + Uvicorn）                 │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐                  │
│  │ Worker 1 │  │ Worker 2 │  │ Worker 3 │                  │
│  │ :8000    │  │ :8000    │  │ :8001    │                  │
│  └──────────┘  └──────────┘  └──────────┘                  │
│                                                              │
│  ┌────────────────────────────────────────────────────┐    │
│  │  API Routers                                       │    │
│  │  ├─ /auth         - 认证登录                      │    │
│  │  ├─ /users        - 用户管理                      │    │
│  │  ├─ /teams        - 团队管理                      │    │
│  │  ├─ /portfolios   - 投资组合                      │    │
│  │  ├─ /profit/*     - 收益管理                      │    │
│  │  ├─ /snapshots    - 数据快照                      │    │
│  │  └─ /health       - 健康检查                      │    │
│  └────────────────────────────────────────────────────┘    │
│                                                              │
│  ┌────────────────────────────────────────────────────┐    │
│  │  Middleware                                        │    │
│  │  ├─ CORS 跨域处理                                 │    │
│  │  ├─ JWT 认证中间件                                │    │
│  │  ├─ 权限验证                                      │    │
│  │  └─ 异常处理                                      │    │
│  └────────────────────────────────────────────────────┘    │
└────────────┬───────────────────┬────────────────────────────┘
             │                   │
             ▼                   ▼
┌─────────────────────┐  ┌──────────────────────────┐
│   数据层（MySQL）    │  │   外部服务集成           │
│  AWS RDS Instance   │  │  ┌──────────────────┐   │
│                     │  │  │ OneToken API     │   │
│  - 用户表           │  │  └──────────────────┘   │
│  - 团队表           │  │  ┌──────────────────┐   │
│  - 投资组合表       │  │  │ Ceffu API        │   │
│  - 收益表           │  │  └──────────────────┘   │
│  - 快照表           │  └──────────────────────────┘
└─────────────────────┘
```

### 2.2 技术栈

#### 后端框架
```
FastAPI 0.104.1      - ASGI Web 框架
Uvicorn 0.24.0       - ASGI 服务器（多进程）
Python 3.12          - 编程语言
```

#### 数据库
```
MySQL 8.0            - 关系型数据库
SQLAlchemy 2.0.23    - ORM 框架
PyMySQL              - MySQL 驱动
```

#### 认证与安全
```
python-jose 3.3.0    - JWT Token 生成/验证
passlib 1.7.4        - 密码加密（bcrypt）
python-multipart     - 表单数据处理
```

#### 数据验证
```
Pydantic 2.5.0       - 数据模型验证
pydantic-settings    - 配置管理
```

#### 工具库
```
httpx                - HTTP 客户端
redis                - 缓存（可选）
celery               - 异步任务（可选）
```

---

## 3. 项目结构

### 3.1 目录结构

```
fund_management_api/
│
├── server/                          # 服务器代码目录
│   ├── app/                         # 应用主目录
│   │   ├── __init__.py
│   │   ├── main.py                  # FastAPI 应用入口
│   │   │
│   │   ├── core/                    # 核心模块
│   │   │   ├── __init__.py
│   │   │   ├── config.py            # 配置管理（Pydantic Settings）
│   │   │   ├── database.py          # 数据库连接管理
│   │   │   └── security.py          # 安全相关（JWT、密码加密）
│   │   │
│   │   ├── models/                  # 数据库模型（SQLAlchemy ORM）
│   │   │   ├── __init__.py
│   │   │   ├── base.py              # Base Model（公共字段）
│   │   │   ├── user.py              # 用户模型
│   │   │   ├── team.py              # 团队模型
│   │   │   ├── portfolio.py         # 投资组合模型
│   │   │   ├── profit.py            # 收益相关模型（10+表）
│   │   │   ├── snapshots.py         # 快照模型
│   │   │   ├── permission.py        # 权限模型
│   │   │   ├── user_session.py      # 用户会话
│   │   │   └── blacklist.py         # 黑名单
│   │   │
│   │   ├── schemas/                 # Pydantic 数据模式（请求/响应）
│   │   │   ├── __init__.py
│   │   │   ├── users.py             # 用户相关 Schema
│   │   │   ├── teams.py             # 团队相关 Schema
│   │   │   ├── portfolios.py        # 投资组合 Schema
│   │   │   ├── profits.py           # 收益相关 Schema
│   │   │   └── responses.py         # 通用响应格式
│   │   │
│   │   ├── api/                     # API 路由
│   │   │   └── routers/             # 各模块路由
│   │   │       ├── __init__.py
│   │   │       ├── auth.py          # 认证路由（登录、注册）
│   │   │       ├── users.py         # 用户管理
│   │   │       ├── teams.py         # 团队管理
│   │   │       ├── portfolios.py    # 投资组合管理
│   │   │       ├── profits.py       # 收益管理（核心）
│   │   │       ├── snapshots.py     # 快照管理
│   │   │       ├── health.py        # 健康检查
│   │   │       ├── onetoken_api.py  # OneToken 集成
│   │   │       └── ceffu_api.py     # Ceffu 集成
│   │   │
│   │   ├── middleware/              # 中间件
│   │   │   ├── __init__.py
│   │   │   ├── auth.py              # JWT 认证中间件
│   │   │   └── cors.py              # CORS 处理
│   │   │
│   │   ├── services/                # 业务逻辑层
│   │   │   ├── __init__.py
│   │   │   ├── user_service.py      # 用户服务
│   │   │   ├── profit_service.py    # 收益计算服务
│   │   │   └── snapshot_service.py  # 快照服务
│   │   │
│   │   ├── utils/                   # 工具函数
│   │   │   ├── __init__.py
│   │   │   ├── jwt_utils.py         # JWT 工具
│   │   │   └── validators.py        # 数据验证
│   │   │
│   │   ├── db/                      # 数据库相关
│   │   │   ├── __init__.py
│   │   │   └── mysql.py             # MySQL 连接池
│   │   │
│   │   ├── auth.py                  # 认证逻辑（独立模块）
│   │   ├── settings.py              # 配置文件
│   │   ├── logging_config.py        # 日志配置
│   │   └── responses.py             # 响应格式化
│   │
│   ├── tests/                       # 测试目录
│   │   ├── __init__.py
│   │   ├── conftest.py              # Pytest 配置
│   │   └── unit/                    # 单元测试
│   │       ├── test_users.py
│   │       └── test_profits.py
│   │
│   ├── migrate.py                   # 数据库迁移脚本
│   ├── requirements.txt             # Python 依赖
│   └── README.md                    # 项目说明
│
├── .env                             # 环境变量配置
├── cedefi-server.pem                # AWS SSH 密钥
└── README.md                        # 主文档
```

### 3.2 核心模块说明

#### 📁 core/ - 核心配置
| 文件 | 职责 |
|------|------|
| `config.py` | 统一配置管理（数据库、JWT、API Keys） |
| `database.py` | 数据库连接池、Session 管理 |
| `security.py` | JWT 生成/验证、密码加密 |

#### 📁 models/ - 数据模型
| 文件 | 管理的表 |
|------|----------|
| `user.py` | users |
| `team.py` | teams |
| `portfolio.py` | portfolios |
| `profit.py` | 10+ 收益相关表 |
| `snapshots.py` | nav_snapshots, rate_snapshots, assets_snapshots |

#### 📁 api/routers/ - API 路由
| 文件 | 端点 | 功能 |
|------|------|------|
| `auth.py` | `/auth/login` | 用户登录 |
| `users.py` | `/users/*` | 用户 CRUD |
| `profits.py` | `/profit/*` | 收益管理（6个核心端点）|
| `portfolios.py` | `/portfolios/*` | 投资组合管理 |

---

## 4. 数据库设计

### 4.1 数据库概览

**数据库**: `fund_management`  
**类型**: MySQL 8.0  
**位置**: AWS RDS（Tokyo Region）  
**总表数**: 22 张表

### 4.2 核心表结构

#### 🔐 用户与权限（4 张表）

##### users - 用户表
```sql
CREATE TABLE users (
    id INT AUTO_INCREMENT PRIMARY KEY,
    email VARCHAR(255) UNIQUE NOT NULL,           -- 邮箱（登录用）
    password_hash VARCHAR(255) NOT NULL,          -- 密码哈希
    is_super BOOLEAN DEFAULT 0,                   -- 超级管理员
    is_active BOOLEAN DEFAULT 1,                  -- 是否激活
    suspended BOOLEAN DEFAULT 0,                  -- 是否禁用
    permissions_json TEXT,                        -- 权限 JSON 数组
    last_login_at DATETIME,                       -- 最后登录时间
    created_at DATETIME,                          -- 创建时间
    updated_at DATETIME                           -- 更新时间
);

-- 索引
INDEX idx_email (email)
```

**关键字段说明**:
- `is_super`: 超级管理员拥有所有权限
- `permissions_json`: 存储为 JSON 数组，如 `["profit:read", "user:write"]`
- `suspended`: 禁用用户无法登录

##### permissions - 权限定义表
```sql
CREATE TABLE permissions (
    id INT AUTO_INCREMENT PRIMARY KEY,
    name VARCHAR(100) UNIQUE NOT NULL,            -- 权限名称
    description TEXT,                             -- 权限描述
    created_at DATETIME
);
```

##### user_permissions - 用户权限关联表
```sql
CREATE TABLE user_permissions (
    id INT AUTO_INCREMENT PRIMARY KEY,
    user_id INT NOT NULL,                         -- 用户 ID
    permission_id INT NOT NULL,                   -- 权限 ID
    created_at DATETIME,
    
    FOREIGN KEY (user_id) REFERENCES users(id),
    FOREIGN KEY (permission_id) REFERENCES permissions(id)
);
```

##### user_sessions - 用户会话表
```sql
CREATE TABLE user_sessions (
    id INT AUTO_INCREMENT PRIMARY KEY,
    user_id INT NOT NULL,                         -- 用户 ID
    token VARCHAR(500) NOT NULL,                  -- JWT Token
    expires_at DATETIME NOT NULL,                 -- 过期时间
    created_at DATETIME,
    
    FOREIGN KEY (user_id) REFERENCES users(id)
);

-- 索引
INDEX idx_token (token),
INDEX idx_user_id (user_id)
```

---

#### 👥 组织结构（2 张表）

##### teams - 团队表
```sql
CREATE TABLE teams (
    id INT PRIMARY KEY,
    name VARCHAR(255),                            -- 团队名称
    created_at DATETIME,
    updated_at DATETIME
);
```

##### portfolios - 投资组合表
```sql
CREATE TABLE portfolios (
    id INT PRIMARY KEY,
    fund_name VARCHAR(255),                       -- 基金名称
    fund_alias VARCHAR(255),                      -- 基金别名
    inception_time DATETIME,                      -- 成立时间
    account_name VARCHAR(255),                    -- 账户名称
    account_alias VARCHAR(255),                   -- 账户别名
    ceffu_wallet_id VARCHAR(255),                 -- Ceffu 钱包 ID
    ceffu_wallet_name VARCHAR(255),               -- Ceffu 钱包名称
    team_id INT,                                  -- 所属团队
    parent_id INT,                                -- 父投资组合
    created_at DATETIME,
    updated_at DATETIME,
    
    FOREIGN KEY (team_id) REFERENCES teams(id),
    FOREIGN KEY (parent_id) REFERENCES portfolios(id)
);
```

**关键概念**:
- 一个 **Team** 可以有多个 **Portfolio**
- Portfolio 可以有层级关系（parent_id）
- 与 Ceffu 钱包关联

---

#### 💰 收益管理（核心业务 - 10 张表）

##### profit_allocation_ratios - 收益分配比例
```sql
CREATE TABLE profit_allocation_ratios (
    id INT PRIMARY KEY,
    portfolio_id INT NOT NULL,                    -- 投资组合 ID
    version INT NOT NULL,                         -- 版本号
    to_team INT NOT NULL,                         -- 团队分配比例（10000=100%）
    to_platform INT NOT NULL,                     -- 平台分配比例
    to_user INT NOT NULL,                         -- 用户分配比例
    created_at DATETIME,
    created_by INT,                               -- 创建者
    updated_at DATETIME,
    updated_by INT,                               -- 更新者
    
    FOREIGN KEY (portfolio_id) REFERENCES portfolios(id),
    FOREIGN KEY (created_by) REFERENCES users(id),
    FOREIGN KEY (updated_by) REFERENCES users(id)
);
```

**分配比例说明**:
```
to_team + to_platform + to_user = 10000 (100%)

示例:
to_team = 7000      (70%)
to_platform = 2000  (20%)
to_user = 1000      (10%)
```

##### acc_profit_from_portfolio - 投资组合累计收益
```sql
CREATE TABLE acc_profit_from_portfolio (
    id INT AUTO_INCREMENT PRIMARY KEY,
    portfolio_id INT NOT NULL,                    -- 投资组合 ID
    snapshot_at BIGINT NOT NULL,                  -- 快照时间戳（秒）
    acc_profit DECIMAL(20,8) NOT NULL,            -- 累计收益（USD）
    created_at DATETIME,
    updated_at DATETIME,
    
    FOREIGN KEY (portfolio_id) REFERENCES portfolios(id),
    INDEX idx_snapshot (snapshot_at)
);
```

##### profit_allocation_logs - 收益分配日志（每小时）
```sql
CREATE TABLE profit_allocation_logs (
    id INT AUTO_INCREMENT PRIMARY KEY,
    portfolio_id INT NOT NULL,                    -- 投资组合 ID
    hour_end_at BIGINT NOT NULL,                  -- 整点时间戳
    
    -- 快照引用
    hourly_snapshot_prev_id INT NOT NULL,         -- 前一小时快照
    hourly_snapshot_curr_id INT NOT NULL,         -- 当前小时快照
    
    -- 收益计算
    hourly_profit DECIMAL(20,8) NOT NULL,         -- 本小时收益
    profit_to_team DECIMAL(20,8) NOT NULL,        -- 分给团队
    profit_to_user DECIMAL(20,8) NOT NULL,        -- 分给用户
    profit_to_platform DECIMAL(20,8) NOT NULL,    -- 分给平台
    
    -- 分配依据
    allocation_ratio_id INT NOT NULL,             -- 使用的分配比例
    
    created_at DATETIME,
    updated_at DATETIME,
    
    FOREIGN KEY (portfolio_id) REFERENCES portfolios(id),
    FOREIGN KEY (allocation_ratio_id) REFERENCES profit_allocation_ratios(id),
    INDEX idx_hour (hour_end_at)
);
```

**收益分配流程**:
```
1. 每小时整点触发
2. 获取当前累计收益 - 前一小时累计收益 = hourly_profit
3. 按 allocation_ratio 分配:
   - profit_to_team = hourly_profit × (to_team / 10000)
   - profit_to_user = hourly_profit × (to_user / 10000)
   - profit_to_platform = hourly_profit × (to_platform / 10000)
```

##### profit_withdrawals - 提现记录
```sql
CREATE TABLE profit_withdrawals (
    id INT PRIMARY KEY,
    from_type VARCHAR(255),                       -- 提现类型（team/platform）
    team_id INT,                                  -- 团队 ID（如果是团队提现）
    chain_id VARCHAR(255),                        -- 区块链 ID
    transaction_hash VARCHAR(255),                -- 交易哈希
    transaction_time DATETIME,                    -- 交易时间
    usd_value VARCHAR(255),                       -- USD 价值
    assets VARCHAR(255),                          -- 资产类型（USDT/USDC）
    assets_amount VARCHAR(255),                   -- 资产数量
    created_at DATETIME,
    
    FOREIGN KEY (team_id) REFERENCES teams(id)
);
```

##### profit_reallocations - 调账记录
```sql
CREATE TABLE profit_reallocations (
    id INT PRIMARY KEY,
    from_type VARCHAR(255),                       -- 转出类型
    to_type VARCHAR(255),                         -- 转入类型
    from_team_id INT,                             -- 转出团队
    to_team_id INT,                               -- 转入团队
    usd_value VARCHAR(255),                       -- 调账金额
    reason TEXT,                                  -- 调账原因
    created_at DATETIME,
    
    FOREIGN KEY (from_team_id) REFERENCES teams(id),
    FOREIGN KEY (to_team_id) REFERENCES teams(id)
);
```

##### 累计收益快照表（3 张）
```sql
-- 用户累计收益
CREATE TABLE acc_profit_user (
    id INT PRIMARY KEY,
    snapshot_at BIGINT NOT NULL,                  -- 快照时间
    acc_profit DECIMAL(20,8) NOT NULL,            -- 累计收益
    created_at DATETIME,
    INDEX idx_snapshot (snapshot_at)
);

-- 平台累计收益
CREATE TABLE acc_profit_platform (
    id INT PRIMARY KEY,
    snapshot_at BIGINT NOT NULL,
    acc_profit DECIMAL(20,8) NOT NULL,
    created_at DATETIME,
    INDEX idx_snapshot (snapshot_at)
);

-- 团队累计收益
CREATE TABLE acc_profit_team (
    id INT PRIMARY KEY,
    portfolio_id INT NOT NULL,
    snapshot_at BIGINT NOT NULL,
    acc_profit DECIMAL(20,8) NOT NULL,
    created_at DATETIME,
    FOREIGN KEY (portfolio_id) REFERENCES portfolios(id),
    INDEX idx_snapshot (snapshot_at)
);
```

---

#### 📊 快照管理（3 张表）

##### nav_snapshots - 净值快照
```sql
CREATE TABLE nav_snapshots (
    id INT AUTO_INCREMENT PRIMARY KEY,
    portfolio_id INT NOT NULL,                    -- 投资组合 ID
    nav_value DECIMAL(20,8) NOT NULL,             -- 净值
    snapshot_time BIGINT NOT NULL,                -- 快照时间戳
    created_at DATETIME,
    
    FOREIGN KEY (portfolio_id) REFERENCES portfolios(id),
    INDEX idx_snapshot (snapshot_time)
);
```

##### rate_snapshots - 汇率快照
```sql
CREATE TABLE rate_snapshots (
    id INT AUTO_INCREMENT PRIMARY KEY,
    currency_pair VARCHAR(20) NOT NULL,           -- 货币对（如 USD/CNY）
    rate DECIMAL(20,8) NOT NULL,                  -- 汇率
    snapshot_time BIGINT NOT NULL,
    created_at DATETIME,
    
    INDEX idx_snapshot (snapshot_time)
);
```

##### assets_snapshots - 资产快照
```sql
CREATE TABLE assets_snapshots (
    id INT AUTO_INCREMENT PRIMARY KEY,
    portfolio_id INT NOT NULL,
    asset_type VARCHAR(50) NOT NULL,              -- 资产类型
    amount DECIMAL(20,8) NOT NULL,                -- 数量
    usd_value DECIMAL(20,8) NOT NULL,             -- USD 价值
    snapshot_time BIGINT NOT NULL,
    created_at DATETIME,
    
    FOREIGN KEY (portfolio_id) REFERENCES portfolios(id),
    INDEX idx_snapshot (snapshot_time)
);
```

---

#### 🔧 其他表（3 张）

##### blacklist - 黑名单
```sql
CREATE TABLE blacklist (
    id INT AUTO_INCREMENT PRIMARY KEY,
    entity_type VARCHAR(50) NOT NULL,             -- 实体类型（user/ip）
    entity_value VARCHAR(255) NOT NULL,           -- 实体值
    reason TEXT,                                  -- 原因
    created_at DATETIME,
    expires_at DATETIME                           -- 过期时间
);
```

##### operation_logs - 操作日志
```sql
CREATE TABLE operation_logs (
    id INT AUTO_INCREMENT PRIMARY KEY,
    user_id INT,                                  -- 操作用户
    action VARCHAR(100) NOT NULL,                 -- 操作类型
    entity_type VARCHAR(50),                      -- 实体类型
    entity_id INT,                                -- 实体 ID
    details TEXT,                                 -- 详细信息（JSON）
    ip_address VARCHAR(50),                       -- IP 地址
    created_at DATETIME,
    
    FOREIGN KEY (user_id) REFERENCES users(id)
);
```

##### subaccounts - 子账户
```sql
CREATE TABLE subaccounts (
    id INT AUTO_INCREMENT PRIMARY KEY,
    portfolio_id INT NOT NULL,                    -- 所属投资组合
    account_name VARCHAR(255) NOT NULL,           -- 账户名称
    exchange VARCHAR(100),                        -- 交易所
    api_key VARCHAR(255),                         -- API Key
    created_at DATETIME,
    
    FOREIGN KEY (portfolio_id) REFERENCES portfolios(id)
);
```

### 4.3 数据库关系图

```
┌─────────────┐
│   users     │◄────────┐
└─────────────┘         │
      │                 │
      │ 1:N             │ created_by
      ▼                 │
┌─────────────┐         │
│user_sessions│         │
└─────────────┘         │
                        │
┌─────────────┐    ┌────┴──────────────┐
│   teams     │◄───│  portfolios       │
└─────────────┘    └───────────────────┘
      │ 1:N              │
      │                  │ 1:N
      │                  ▼
      │            ┌──────────────────────────┐
      │            │profit_allocation_ratios  │
      │            └──────────────────────────┘
      │                  │
      │                  │ used_in
      │                  ▼
      │            ┌──────────────────────────┐
      │            │profit_allocation_logs    │◄─┐
      │            └──────────────────────────┘  │
      │                  │                       │
      │                  │                       │
      │                  ▼                       │ references
      │            ┌──────────────────────────┐  │
      │            │acc_profit_from_portfolio │──┘
      │            └──────────────────────────┘
      │
      └──────────► ┌──────────────────────────┐
                   │profit_withdrawals        │
                   └──────────────────────────┘
                   ┌──────────────────────────┐
                   │profit_reallocations      │
                   └──────────────────────────┘
```

---

## 5. 核心业务逻辑

### 5.1 收益分配流程

```
┌───────────────────────────────────────────────────────────────┐
│                    收益分配完整流程                            │
└───────────────────────────────────────────────────────────────┘

Step 1: 数据采集（每小时执行）
├─ 从 Ceffu API 获取钱包余额
├─ 从 OneToken API 获取交易数据
└─ 计算投资组合当前总资产价值

Step 2: 快照生成
├─ 记录到 acc_profit_from_portfolio
├─ snapshot_at = 当前整点时间戳
└─ acc_profit = 当前总资产 - 初始投资

Step 3: 计算小时收益
├─ hourly_profit = 当前累计收益 - 上一小时累计收益
└─ 获取当前生效的 profit_allocation_ratios

Step 4: 分配收益
├─ profit_to_team = hourly_profit × (to_team / 10000)
├─ profit_to_platform = hourly_profit × (to_platform / 10000)
└─ profit_to_user = hourly_profit × (to_user / 10000)

Step 5: 记录分配日志
├─ 写入 profit_allocation_logs
├─ 关联 prev_snapshot 和 curr_snapshot
└─ 记录使用的 allocation_ratio_id

Step 6: 更新累计账户
├─ 更新 acc_profit_team
├─ 更新 acc_profit_platform
└─ 更新 acc_profit_user

Step 7: 通知
└─ 触发通知（可选）
```

### 5.2 提现流程

```
┌───────────────────────────────────────────────────────────────┐
│                        提现流程                                │
└───────────────────────────────────────────────────────────────┘

1. 用户/管理员发起提现请求
   ├─ 指定 from_type (team/platform)
   ├─ 指定 team_id（如果是团队提现）
   ├─ 指定提现金额和资产类型
   └─ 权限验证

2. 系统验证
   ├─ 检查账户余额是否充足
   ├─ 检查用户权限
   └─ 验证提现地址

3. 调用 Ceffu API
   ├─ 创建链上提现交易
   ├─ 获取 transaction_hash
   └─ 等待交易确认

4. 记录提现
   ├─ 写入 profit_withdrawals
   ├─ 记录 chain_id, tx_hash, timestamp
   └─ 记录 assets, assets_amount, usd_value

5. 更新账户余额
   ├─ 减少对应账户的累计收益
   └─ 记录 delta_from_withdraw

6. 通知
   └─ 发送提现成功通知
```

### 5.3 调账流程

```
┌───────────────────────────────────────────────────────────────┐
│                        调账流程                                │
└───────────────────────────────────────────────────────────────┘

使用场景: 
- 补偿用户损失
- 团队间转账
- 纠正错误分配

流程:
1. 管理员发起调账
   ├─ from_type, to_type (team/user/platform)
   ├─ from_team_id, to_team_id
   ├─ usd_value (调账金额)
   └─ reason (原因说明)

2. 权限验证
   └─ 需要超级管理员权限

3. 记录调账
   └─ 写入 profit_reallocations

4. 更新账户
   ├─ 减少 from 账户余额
   ├─ 增加 to 账户余额
   └─ 记录 delta_from_reallocation

5. 审计日志
   └─ 记录到 operation_logs
```

### 5.4 认证与授权流程

```
┌───────────────────────────────────────────────────────────────┐
│                   JWT 认证流程                                 │
└───────────────────────────────────────────────────────────────┘

1. 用户登录
   POST /auth/login
   ├─ 提交 email + password
   └─ 服务器验证

2. 验证密码
   ├─ 查询 users 表
   ├─ bcrypt.verify(password, password_hash)
   └─ 验证是否被禁用 (suspended)

3. 生成 JWT Token
   ├─ Payload: {user_id, email, is_super, permissions}
   ├─ 使用 SECRET_KEY 签名
   ├─ 设置过期时间（30分钟）
   └─ 算法: HS256

4. 返回 Token
   {
     "token": "eyJhbGc...",
     "user": {...}
   }

5. 后续请求
   ├─ Header: Authorization: Bearer <token>
   ├─ 中间件验证 Token
   ├─ 解析 Payload
   ├─ 检查权限
   └─ 允许/拒绝访问
```

**权限检查逻辑**:
```python
def check_permission(user, required_permission):
    # 超级管理员拥有所有权限
    if user.is_super:
        return True
    
    # 检查用户权限列表
    if required_permission in user.permissions:
        return True
    
    return False
```

---

## 6. AWS 集成

### 6.1 AWS 架构

```
┌─────────────────────────────────────────────────────────────┐
│                        AWS 架构                              │
└─────────────────────────────────────────────────────────────┘

Region: ap-northeast-1 (Tokyo)

┌──────────────────────────────────────────────────────────────┐
│  VPC (Virtual Private Cloud)                                 │
│                                                               │
│  ┌────────────────────────────────────────────────────────┐  │
│  │  Public Subnet                                         │  │
│  │                                                         │  │
│  │  ┌───────────────────────────────────────┐            │  │
│  │  │  EC2 Instance                         │            │  │
│  │  │  - Type: t2.micro / t3.micro         │            │  │
│  │  │  - Public IP: 13.113.11.170          │            │  │
│  │  │  - OS: Ubuntu 20.04/22.04            │            │  │
│  │  │  - Services:                          │            │  │
│  │  │    * Nginx (Port 80)                 │            │  │
│  │  │    * Uvicorn (Port 8000, 8001)       │            │  │
│  │  │    * Python 3.12                     │            │  │
│  │  └───────────────────────────────────────┘            │  │
│  │                                                         │  │
│  │  Security Group: sg-xxxxxx                            │  │
│  │  - Inbound: 80 (HTTP), 22 (SSH)                      │  │
│  │  - Outbound: All                                      │  │
│  └────────────────────────────────────────────────────────┘  │
│                                                               │
│  ┌────────────────────────────────────────────────────────┐  │
│  │  Private Subnet                                        │  │
│  │                                                         │  │
│  │  ┌───────────────────────────────────────┐            │  │
│  │  │  RDS MySQL Instance                   │            │  │
│  │  │  - Engine: MySQL 8.0                  │            │  │
│  │  │  - Instance: db.t3.micro              │            │  │
│  │  │  - Endpoint:                          │            │  │
│  │  │    cedefi-database-instance.          │            │  │
│  │  │    cwwyatalynow.ap-northeast-1.       │            │  │
│  │  │    rds.amazonaws.com                  │            │  │
│  │  │  - Port: 49123 (custom)               │            │  │
│  │  │  - Storage: 20GB SSD                  │            │  │
│  │  │  - Multi-AZ: No                       │            │  │
│  │  └───────────────────────────────────────┘            │  │
│  │                                                         │  │
│  │  Security Group: sg-yyyyyy                            │  │
│  │  - Inbound: 49123 (from EC2 SG)                      │  │
│  └────────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────┘
```

### 6.2 EC2 实例配置

**实例信息**:
```
Instance ID: i-xxxxxxxxx
Type: t2.micro / t3.micro
vCPU: 1-2 核
Memory: 1GB
Storage: 8GB EBS
Public IP: 13.113.11.170
```

**部署的服务**:
```
1. Nginx (反向代理)
   - Port: 80
   - Upstream: localhost:8000, localhost:8001
   - Config: /etc/nginx/sites-available/fund-api

2. Uvicorn Workers
   - Worker 1-2: Port 8000
   - Worker 3: Port 8001
   - Process Manager: systemd / supervisor

3. Python Environment
   - Version: 3.12
   - Virtual Env: /home/ubuntu/fund_management_api/venv
```

### 6.3 RDS 数据库配置

**连接信息**:
```
Endpoint: cedefi-database-instance.cwwyatalynow.ap-northeast-1.rds.amazonaws.com
Port: 49123
Database: fund_management
User: admin
```

**性能配置**:
```
Instance Class: db.t3.micro
Storage: 20 GB SSD (gp2)
Max Connections: 100
Character Set: utf8mb4
Collation: utf8mb4_unicode_ci
```

**备份策略**:
```
Automated Backup: 启用
Backup Window: 03:00-04:00 UTC
Retention: 7 天
Snapshot: 手动快照可用
```

### 6.4 安全组配置

**EC2 Security Group**:
| 类型 | 协议 | 端口 | 源 | 说明 |
|------|------|------|-----|------|
| Inbound | TCP | 22 | 你的IP | SSH 访问 |
| Inbound | TCP | 80 | 0.0.0.0/0 | HTTP API |
| Outbound | All | All | 0.0.0.0/0 | 所有出站 |

**RDS Security Group**:
| 类型 | 协议 | 端口 | 源 | 说明 |
|------|------|------|-----|------|
| Inbound | TCP | 49123 | EC2-SG | 允许 EC2 访问 |

---

## 7. 外部 API 集成

### 7.1 OneToken API

**用途**: 交易数据获取

**配置**:
```python
ONETOKEN_API_KEY = os.getenv("ONETOKEN_API_KEY")
ONETOKEN_API_SECRET = os.getenv("ONETOKEN_API_SECRET")
ONETOKEN_BASE_URL = "https://api.onetoken.trade"
```

**主要功能**:
- 获取交易所账户余额
- 查询交易历史
- 获取市场行情
- 执行交易（可选）

**集成方式**:
```python
# server/app/api/routers/onetoken_api.py

async def get_account_balance(account: str):
    """获取 OneToken 账户余额"""
    url = f"{ONETOKEN_BASE_URL}/v1/accounts/{account}/balance"
    headers = {
        "Authorization": f"Bearer {generate_onetoken_token()}"
    }
    response = await httpx.get(url, headers=headers)
    return response.json()
```

### 7.2 Ceffu API

**用途**: 托管钱包管理

**配置**:
```python
CEFFU_API_KEY = os.getenv("CEFFU_API_KEY")
CEFFU_SECRET_KEY = os.getenv("CEFFU_SECRET_KEY")
CEFFU_BASE_URL = "https://api.ceffu.com"
```

**主要功能**:
- 查询钱包余额
- 获取存款地址
- 执行提现
- 查询交易记录

**集成方式**:
```python
# server/app/api/routers/ceffu_api.py

async def get_wallet_balance(wallet_id: str):
    """获取 Ceffu 钱包余额"""
    timestamp = int(time.time() * 1000)
    signature = generate_ceffu_signature(wallet_id, timestamp)
    
    url = f"{CEFFU_BASE_URL}/v1/wallets/{wallet_id}/balance"
    headers = {
        "X-API-KEY": CEFFU_API_KEY,
        "X-TIMESTAMP": str(timestamp),
        "X-SIGNATURE": signature
    }
    response = await httpx.get(url, headers=headers)
    return response.json()
```

**签名算法**:
```python
def generate_ceffu_signature(params: dict, timestamp: int) -> str:
    """生成 Ceffu API 签名"""
    message = f"{timestamp}{json.dumps(params, sort_keys=True)}"
    signature = hmac.new(
        CEFFU_SECRET_KEY.encode(),
        message.encode(),
        hashlib.sha256
    ).hexdigest()
    return signature
```

---

## 8. 安全机制

### 8.1 认证安全

**JWT Token**:
```python
# 生成 Token
def create_access_token(user_id: int, user_data: dict):
    payload = {
        "user_id": user_id,
        "email": user_data["email"],
        "is_super": user_data.get("is_super", False),
        "permissions": user_data.get("permissions", []),
        "exp": datetime.utcnow() + timedelta(minutes=30),
        "iat": datetime.utcnow()
    }
    token = jwt.encode(payload, SECRET_KEY, algorithm="HS256")
    return token

# 验证 Token
def verify_token(token: str):
    try:
        payload = jwt.decode(token, SECRET_KEY, algorithms=["HS256"])
        return payload
    except jwt.ExpiredSignatureError:
        raise HTTPException(401, "Token 已过期")
    except jwt.JWTError:
        raise HTTPException(401, "无效的 Token")
```

**密码加密**:
```python
from passlib.context import CryptContext

pwd_context = CryptContext(schemes=["bcrypt"], deprecated="auto")

# 加密密码
hashed = pwd_context.hash("password123")

# 验证密码
is_valid = pwd_context.verify("password123", hashed)
```

### 8.2 API 安全

**CORS 配置**:
```python
from fastapi.middleware.cors import CORSMiddleware

app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],  # 生产环境应限制具体域名
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)
```

**权限验证**:
```python
def require_permission(permission: str):
    """权限装饰器"""
    def decorator(func):
        async def wrapper(*args, **kwargs):
            user = kwargs.get("current_user")
            if not user.has_permission(permission):
                raise HTTPException(403, "权限不足")
            return await func(*args, **kwargs)
        return wrapper
    return decorator

# 使用
@app.get("/admin/users")
@require_permission("user:read")
async def get_users(current_user: User = Depends(get_current_user)):
    pass
```

### 8.3 数据安全

**SQL 注入防护**:
- 使用 SQLAlchemy ORM（参数化查询）
- 禁止直接拼接 SQL

**敏感数据保护**:
```python
class User(Base):
    def to_dict(self):
        data = super().to_dict()
        # 移除敏感字段
        data.pop('password_hash', None)
        data.pop('permissions_json', None)
        return data
```

**环境变量管理**:
```bash
# .env 文件（不提交到 Git）
MYSQL_PASSWORD=***
SECRET_KEY=***
CEFFU_API_KEY=***
```

---

## 📞 总结

### 核心特点

✅ **FastAPI 框架** - 高性能、自动文档  
✅ **JWT 认证** - 无状态、可扩展  
✅ **细粒度权限** - 灵活的权限控制  
✅ **收益分配** - 自动化小时结算  
✅ **AWS 部署** - EC2 + RDS  
✅ **外部集成** - OneToken + Ceffu  
✅ **数据快照** - 完整的历史记录  
✅ **安全设计** - 多层安全防护  

### 技术亮点

1. **SQLAlchemy ORM** - 类型安全、易维护
2. **Pydantic 验证** - 自动数据验证
3. **异步架构** - 高并发支持
4. **模块化设计** - 清晰的代码结构
5. **标准化响应** - 统一的 API 格式

---

**文档版本**: v1.0  
**最后更新**: 2025-10-17  
**维护团队**: Lucien Team
