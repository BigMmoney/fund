# 🏦 Fund Management API

**专业的基金管理系统后端服务**

[![Python](https://img.shields.io/badge/Python-3.12-blue.svg)](https://www.python.org/)
[![FastAPI](https://img.shields.io/badge/FastAPI-0.104.1-green.svg)](https://fastapi.tiangolo.com/)
[![MySQL](https://img.shields.io/badge/MySQL-8.0-orange.svg)](https://www.mysql.com/)
[![Status](https://img.shields.io/badge/Status-Production-success.svg)](http://13.113.11.170)

一个功能完善的基金管理系统，提供用户管理、投资组合管理、收益分配、提现调账等核心功能，集成 OneToken 和 Ceffu API。

---

## 📋 目录

- [项目概述](#-项目概述)
- [核心功能](#-核心功能)
- [技术架构](#-技术架构)
- [快速开始](#-快速开始)
- [API 文档](#-api-文档)
- [项目结构](#-项目结构)
- [数据库设计](#-数据库设计)
- [部署说明](#-部署说明)
- [开发指南](#-开发指南)
- [安全说明](#-安全说明)
- [常见问题](#-常见问题)
- [更新日志](#-更新日志)

---

## 🎯 项目概述

### 项目信息

| 项目 | 信息 |
|------|------|
| **项目名称** | Fund Management API |
| **版本** | v1.0.0 |
| **状态** | ✅ 生产环境运行中 |
| **系统健康度** | 98% |
| **API 可用率** | 100% (6/6 核心端点) |
| **服务器** | AWS EC2 (Tokyo) |
| **访问地址** | http://13.113.11.170 |

### 项目定位

Fund Management API 是一个企业级的基金管理系统后端服务，专注于：

- 💼 **投资组合管理** - 多层级投资组合结构
- 💰 **自动化收益分配** - 按小时自动结算和分配收益
- 👥 **多租户支持** - 团队隔离和权限管理
- 🔗 **外部集成** - OneToken 和 Ceffu API 无缝集成
- 📊 **数据快照** - 完整的历史数据追踪
- 🔒 **企业级安全** - JWT 认证 + 细粒度权限控制

---

## ⚡ 核心功能

### 1. 用户与权限管理

```
✅ 用户注册与登录（JWT Token）
✅ 超级管理员 + 细粒度权限
✅ 用户会话管理
✅ 密码加密（bcrypt）
✅ 权限验证中间件
```

**API 端点**:
- `POST /auth/login` - 用户登录
- `POST /auth/register` - 用户注册（管理员）
- `GET /users` - 获取用户列表
- `POST /users` - 创建用户
- `PUT /users/{id}` - 更新用户
- `DELETE /users/{id}` - 删除用户

### 2. 投资组合管理

```
✅ 多层级投资组合结构
✅ 团队关联
✅ Ceffu 钱包绑定
✅ 投资组合 CRUD 操作
```

**API 端点**:
- `GET /portfolios` - 获取投资组合列表
- `GET /portfolios/{id}` - 获取投资组合详情
- `POST /portfolios` - 创建投资组合
- `PUT /portfolios/{id}` - 更新投资组合
- `DELETE /portfolios/{id}` - 删除投资组合

### 3. 收益管理（核心）

```
✅ 自动化小时收益结算
✅ 三方收益分配（团队/平台/用户）
✅ 收益分配比例配置
✅ 累计收益快照
✅ 收益分配日志
✅ 提现管理
✅ 调账管理
```

**核心 API 端点**:
- `GET /profit/allocation_ratios` - 获取分配比例
- `POST /profit/allocation_ratios` - 设置分配比例
- `GET /profit/allocation_logs` - 获取分配日志
- `GET /profit/withdrawals` - 获取提现记录
- `POST /profit/withdrawals` - 创建提现
- `GET /profit/reallocations` - 获取调账记录
- `POST /profit/reallocations` - 创建调账

### 4. 数据快照

```
✅ 净值快照（NAV Snapshots）
✅ 汇率快照（Rate Snapshots）
✅ 资产快照（Assets Snapshots）
✅ 收益快照（Profit Snapshots）
✅ 按时间序列查询
```

### 5. 外部集成

```
✅ OneToken API - 交易数据获取
✅ Ceffu API - 托管钱包管理
✅ 自动数据同步
✅ 异常处理和重试机制
```

---

## 🏗️ 技术架构

### 架构图

```
┌─────────────────────────────────────────────────────┐
│                    客户端层                          │
│    Web前端 / 移动端 / 第三方应用 / 管理工具         │
└────────────────────┬────────────────────────────────┘
                     │ HTTPS/HTTP
                     ▼
┌─────────────────────────────────────────────────────┐
│               Nginx 反向代理 (Port 80)              │
│        负载均衡 → Uvicorn (8000, 8001)              │
└────────────────────┬────────────────────────────────┘
                     │
                     ▼
┌─────────────────────────────────────────────────────┐
│          FastAPI 应用层 (Multi-Worker)              │
│  ┌─────────────────────────────────────────────┐   │
│  │  API Routers                                │   │
│  │  - /auth      认证                          │   │
│  │  - /users     用户管理                      │   │
│  │  - /teams     团队管理                      │   │
│  │  - /portfolios 投资组合                     │   │
│  │  - /profit/*   收益管理 ⭐                  │   │
│  │  - /snapshots  数据快照                     │   │
│  │  - /health     健康检查                     │   │
│  └─────────────────────────────────────────────┘   │
│  ┌─────────────────────────────────────────────┐   │
│  │  Middleware                                 │   │
│  │  - CORS / JWT Auth / Permissions / Logging │   │
│  └─────────────────────────────────────────────┘   │
└────────┬──────────────────┬─────────────────────────┘
         │                  │
         ▼                  ▼
┌──────────────────┐  ┌────────────────────┐
│  MySQL 8.0       │  │  External APIs     │
│  AWS RDS         │  │  - OneToken        │
│  - 22 Tables     │  │  - Ceffu           │
│  - Relationships │  └────────────────────┘
└──────────────────┘
```

### 技术栈

#### 后端核心
| 技术 | 版本 | 用途 |
|------|------|------|
| **Python** | 3.12 | 编程语言 |
| **FastAPI** | 0.104.1 | Web 框架 |
| **Uvicorn** | 0.24.0 | ASGI 服务器 |
| **SQLAlchemy** | 2.0.23 | ORM 框架 |
| **Pydantic** | 2.5.0 | 数据验证 |

#### 数据库与缓存
| 技术 | 版本 | 用途 |
|------|------|------|
| **MySQL** | 8.0 | 主数据库 |
| **PyMySQL** | - | MySQL 驱动 |
| **Redis** | 7.x | 缓存（可选）|

#### 安全与认证
| 技术 | 版本 | 用途 |
|------|------|------|
| **python-jose** | 3.3.0 | JWT Token |
| **passlib** | 1.7.4 | 密码加密 |
| **bcrypt** | - | 哈希算法 |

#### 部署与运维
| 技术 | 版本 | 用途 |
|------|------|------|
| **Nginx** | 1.18+ | 反向代理 |
| **AWS EC2** | - | 应用服务器 |
| **AWS RDS** | - | 数据库服务 |
| **systemd** | - | 进程管理 |

---

## 🚀 快速开始

### 前置要求

- Python 3.12+
- MySQL 8.0+
- Redis 7.x（可选）
- Git

### 1. 克隆项目

```bash
git clone <repository-url>
cd fund_management_api
```

### 2. 创建虚拟环境

```bash
# 创建虚拟环境
python -m venv venv

# 激活虚拟环境
# Windows
venv\Scripts\activate
# Linux/Mac
source venv/bin/activate
```

### 3. 安装依赖

```bash
pip install -r server/requirements.txt
```

### 4. 配置环境变量

创建 `.env` 文件：

```bash
# 数据库配置
MYSQL_HOST=localhost
MYSQL_PORT=3306
MYSQL_USER=root
MYSQL_PASSWORD=your_password
MYSQL_DATABASE=fund_management

# JWT 配置
SECRET_KEY=your-secret-key-at-least-32-characters
ALGORITHM=HS256
ACCESS_TOKEN_EXPIRE_MINUTES=30

# OneToken API
ONETOKEN_API_KEY=your_onetoken_key
ONETOKEN_API_SECRET=your_onetoken_secret

# Ceffu API
CEFFU_API_KEY=your_ceffu_key
CEFFU_API_SECRET=your_ceffu_secret

# Redis（可选）
REDIS_HOST=localhost
REDIS_PORT=6379
REDIS_DB=0
```

### 5. 初始化数据库

```bash
cd server
python migrate.py
```

这将：
- ✅ 创建所有数据库表
- ✅ 创建默认管理员账户
- ✅ 初始化基础数据

**默认管理员账户**:
```
Email: admin@example.com
Password: admin123
⚠️ 首次登录后请立即修改密码！
```

### 6. 启动服务

#### 开发环境

```bash
# 单进程模式
uvicorn server.app.main:app --reload --host 0.0.0.0 --port 8000
```

#### 生产环境

```bash
# 多进程模式
uvicorn server.app.main:app --host 0.0.0.0 --port 8000 --workers 4
```

### 7. 访问 API

- **Swagger 文档**: http://localhost:8000/docs
- **ReDoc 文档**: http://localhost:8000/redoc
- **健康检查**: http://localhost:8000/health

---

## 📚 API 文档

### 在线文档

生产环境:
- **Swagger UI**: http://13.113.11.170/docs
- **ReDoc**: http://13.113.11.170/redoc

本地环境:
- **Swagger UI**: http://localhost:8000/docs
- **ReDoc**: http://localhost:8000/redoc

### 快速开始示例

#### 1. 用户登录

```bash
curl -X POST "http://13.113.11.170/auth/login" \
  -H "Content-Type: application/json" \
  -d '{
    "email": "admin@example.com",
    "password": "admin123"
  }'
```

**响应**:
```json
{
  "code": 0,
  "message": "success",
  "data": {
    "token": "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9...",
    "user": {
      "id": 1,
      "email": "admin@example.com",
      "is_super": true
    }
  }
}
```

#### 2. 获取收益分配比例

```bash
TOKEN="your-jwt-token"

curl -X GET "http://13.113.11.170/profit/allocation_ratios?portfolio_id=1" \
  -H "Authorization: Bearer $TOKEN"
```

#### 3. JavaScript/TypeScript 示例

```typescript
const API_BASE = 'http://13.113.11.170';

// 登录
async function login(email: string, password: string) {
  const response = await fetch(`${API_BASE}/auth/login`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ email, password })
  });
  const data = await response.json();
  return data.data.token;
}

// 调用 API
async function getAllocationRatios(portfolioId: number, token: string) {
  const response = await fetch(
    `${API_BASE}/profit/allocation_ratios?portfolio_id=${portfolioId}`,
    {
      headers: { 'Authorization': `Bearer ${token}` }
    }
  );
  return await response.json();
}

// 使用
const token = await login('admin@example.com', 'admin123');
const ratios = await getAllocationRatios(1, token);
```

#### 4. Python 示例

```python
import requests

API_BASE = 'http://13.113.11.170'

# 登录
def login(email: str, password: str) -> str:
    response = requests.post(
        f'{API_BASE}/auth/login',
        json={'email': email, 'password': password}
    )
    return response.json()['data']['token']

# 调用 API
def get_allocation_ratios(portfolio_id: int, token: str):
    response = requests.get(
        f'{API_BASE}/profit/allocation_ratios',
        params={'portfolio_id': portfolio_id},
        headers={'Authorization': f'Bearer {token}'}
    )
    return response.json()

# 使用
token = login('admin@example.com', 'admin123')
ratios = get_allocation_ratios(1, token)
```

### 详细文档

- 📘 [API 完整文档](./API_DOCUMENTATION_COMPLETE.md) - 所有端点详解
- 📗 [API 快速开始](./API_QUICK_START.md) - 5分钟上手指南
- 📕 [项目技术文档](./PROJECT_TECHNICAL_DOCUMENTATION.md) - 技术架构详解
- 📙 [项目架构文档](./PROJECT_ARCHITECTURE.md) - 系统架构设计

---

## 📁 项目结构

```
fund_management_api/
│
├── server/                          # 服务器代码
│   ├── app/                         # 应用主目录
│   │   ├── main.py                  # FastAPI 入口
│   │   ├── auth.py                  # 认证逻辑
│   │   ├── settings.py              # 配置管理
│   │   │
│   │   ├── core/                    # 核心模块
│   │   │   ├── config.py            # 配置（Pydantic）
│   │   │   ├── database.py          # 数据库连接
│   │   │   └── security.py          # 安全（JWT/密码）
│   │   │
│   │   ├── models/                  # 数据模型（ORM）
│   │   │   ├── base.py              # 基础模型
│   │   │   ├── user.py              # 用户模型
│   │   │   ├── team.py              # 团队模型
│   │   │   ├── portfolio.py         # 投资组合
│   │   │   ├── profit.py            # 收益模型（10+表）
│   │   │   ├── snapshots.py         # 快照模型
│   │   │   └── ...
│   │   │
│   │   ├── schemas/                 # 数据模式（Pydantic）
│   │   │   ├── users.py             # 用户 Schema
│   │   │   ├── portfolios.py        # 投资组合 Schema
│   │   │   ├── profits.py           # 收益 Schema
│   │   │   └── responses.py         # 响应格式
│   │   │
│   │   ├── api/routers/             # API 路由
│   │   │   ├── auth.py              # 认证路由
│   │   │   ├── users.py             # 用户管理
│   │   │   ├── teams.py             # 团队管理
│   │   │   ├── portfolios.py        # 投资组合
│   │   │   ├── profits.py           # 收益管理 ⭐
│   │   │   ├── snapshots.py         # 快照管理
│   │   │   ├── health.py            # 健康检查
│   │   │   ├── onetoken_api.py      # OneToken 集成
│   │   │   └── ceffu_api.py         # Ceffu 集成
│   │   │
│   │   ├── middleware/              # 中间件
│   │   │   ├── auth.py              # JWT 认证
│   │   │   └── cors.py              # CORS 处理
│   │   │
│   │   ├── services/                # 业务逻辑层
│   │   │   ├── user_service.py      # 用户服务
│   │   │   ├── profit_service.py    # 收益计算
│   │   │   └── snapshot_service.py  # 快照服务
│   │   │
│   │   └── utils/                   # 工具函数
│   │       ├── jwt_utils.py         # JWT 工具
│   │       └── validators.py        # 验证器
│   │
│   ├── tests/                       # 测试
│   │   ├── conftest.py              # Pytest 配置
│   │   └── unit/                    # 单元测试
│   │
│   ├── migrate.py                   # 数据库迁移
│   ├── requirements.txt             # 依赖
│   └── README.md
│
├── docs/                            # 文档目录
│   ├── API_DOCUMENTATION_COMPLETE.md
│   ├── API_QUICK_START.md
│   ├── PROJECT_ARCHITECTURE.md
│   └── PROJECT_TECHNICAL_DOCUMENTATION.md
│
├── .env                             # 环境变量（不提交）
├── .env.example                     # 环境变量模板
├── .gitignore
├── cedefi-server.pem                # AWS SSH 密钥（不提交）
└── README.md                        # 本文件
```

---

## 💾 数据库设计

### 数据库概览

- **数据库名**: `fund_management`
- **引擎**: MySQL 8.0
- **字符集**: utf8mb4
- **总表数**: 22 张

### 核心表分类

#### 🔐 用户与权限（4张表）

| 表名 | 说明 | 关键字段 |
|------|------|----------|
| `users` | 用户表 | id, email, password_hash, is_super |
| `permissions` | 权限定义 | id, name, description |
| `user_permissions` | 用户权限关联 | user_id, permission_id |
| `user_sessions` | 用户会话 | user_id, token, expires_at |

#### 👥 组织结构（2张表）

| 表名 | 说明 | 关键字段 |
|------|------|----------|
| `teams` | 团队表 | id, name |
| `portfolios` | 投资组合 | id, fund_name, team_id, ceffu_wallet_id |

#### 💰 收益管理（10张表）⭐

| 表名 | 说明 | 用途 |
|------|------|------|
| `profit_allocation_ratios` | 分配比例配置 | 定义收益如何分配 |
| `profit_allocation_logs` | 分配日志 | 记录每小时分配结果 |
| `acc_profit_from_portfolio` | 投资组合累计收益 | 快照记录 |
| `acc_profit_team` | 团队累计收益 | 快照记录 |
| `acc_profit_platform` | 平台累计收益 | 快照记录 |
| `acc_profit_user` | 用户累计收益 | 快照记录 |
| `profit_withdrawals` | 提现记录 | 链上交易记录 |
| `profit_reallocations` | 调账记录 | 虚拟账户调整 |
| `hourly_profit_team` | 团队小时收益 | 增量记录 |
| `hourly_profit_user` | 用户小时收益 | 增量记录 |

#### 📊 快照管理（3张表）

| 表名 | 说明 |
|------|------|
| `nav_snapshots` | 净值快照 |
| `rate_snapshots` | 汇率快照 |
| `assets_snapshots` | 资产快照 |

#### 🔧 其他（3张表）

| 表名 | 说明 |
|------|------|
| `blacklist` | 黑名单 |
| `operation_logs` | 操作日志 |
| `subaccounts` | 子账户 |

### 数据库关系

```
users (1) ──────(N) user_permissions
  │
  └──────(N) user_sessions
  │
  └──────(1) created portfolios
  │
  └──────(1) created profit_allocation_ratios

teams (1) ──────(N) portfolios
  │
  └──────(1) profit_withdrawals
  │
  └──────(1) profit_reallocations

portfolios (1) ──────(N) profit_allocation_ratios
  │
  ├──────(N) profit_allocation_logs
  ├──────(N) acc_profit_from_portfolio
  └──────(N) subaccounts
```

### ER 图简化版

```
┌─────────┐       ┌──────────────┐       ┌──────────────────┐
│  users  │───┬───│  portfolios  │───────│ allocation_ratios│
└─────────┘   │   └──────────────┘       └──────────────────┘
              │          │                        │
              │          │                        │
              │   ┌──────┴────────┐              │
              │   │ allocation_   │◄─────────────┘
              │   │    logs       │
              │   └───────────────┘
              │
              │   ┌──────────────┐
              └───│ profit_      │
                  │ withdrawals  │
                  └──────────────┘
```

### 详细表结构

完整的表结构和字段说明请参考：
- 📘 [项目架构文档](./PROJECT_ARCHITECTURE.md) - 第4章 数据库设计

---

## 🚢 部署说明

### 生产环境配置

**服务器信息**:
```
Provider: AWS EC2
Region: ap-northeast-1 (Tokyo)
Instance: t2.micro / t3.micro
OS: Ubuntu 20.04/22.04
Public IP: 13.113.11.170
```

**数据库信息**:
```
Provider: AWS RDS
Engine: MySQL 8.0
Instance: db.t3.micro
Endpoint: cedefi-database-instance.cwwyatalynow.ap-northeast-1.rds.amazonaws.com
Port: 49123
```

### 部署架构

```
Internet
   │
   ▼
┌─────────────────┐
│ Nginx (Port 80) │
└────────┬────────┘
         │
    ┌────┴────┐
    ▼         ▼
┌────────┐ ┌────────┐
│Uvicorn │ │Uvicorn │
│ :8000  │ │ :8001  │
│(2 workers)│  │(1 worker)│
└────────┘ └────────┘
    │         │
    └────┬────┘
         ▼
┌─────────────────┐
│  AWS RDS MySQL  │
└─────────────────┘
```

### Nginx 配置

```nginx
# /etc/nginx/sites-available/fund-api

upstream fund_api {
    server 127.0.0.1:8000;
    server 127.0.0.1:8001;
}

server {
    listen 80;
    server_name 13.113.11.170;

    location / {
        proxy_pass http://fund_api;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

### 启动服务

#### 方式1: 直接启动

```bash
# Worker 1-2 (Port 8000)
nohup uvicorn server.app.main:app \
  --host 0.0.0.0 \
  --port 8000 \
  --workers 2 \
  > /tmp/uvicorn_8000.log 2>&1 &

# Worker 3 (Port 8001)
nohup uvicorn server.app.main:app \
  --host 0.0.0.0 \
  --port 8001 \
  --workers 1 \
  > /tmp/uvicorn_8001.log 2>&1 &
```

#### 方式2: systemd 服务

创建 `/etc/systemd/system/fund-api.service`:

```ini
[Unit]
Description=Fund Management API
After=network.target

[Service]
Type=simple
User=ubuntu
WorkingDirectory=/home/ubuntu/fund_management_api
Environment="PATH=/home/ubuntu/fund_management_api/venv/bin"
ExecStart=/home/ubuntu/fund_management_api/venv/bin/uvicorn server.app.main:app --host 0.0.0.0 --port 8000 --workers 3
Restart=always

[Install]
WantedBy=multi-user.target
```

启动服务:
```bash
sudo systemctl enable fund-api
sudo systemctl start fund-api
sudo systemctl status fund-api
```

### 监控和日志

```bash
# 查看 Uvicorn 日志
tail -f /tmp/uvicorn_8000.log
tail -f /tmp/uvicorn_8001.log

# 查看 Nginx 日志
sudo tail -f /var/log/nginx/access.log
sudo tail -f /var/log/nginx/error.log

# 检查进程
pgrep -af uvicorn
ps aux | grep uvicorn

# 检查端口
netstat -tlnp | grep -E '8000|8001|80'
```

---

## 👨‍💻 开发指南

### 开发环境设置

1. **安装开发依赖**
```bash
pip install -r server/requirements.txt
pip install pytest pytest-cov black flake8 mypy
```

2. **代码格式化**
```bash
# 使用 black 格式化
black server/app/

# 检查代码风格
flake8 server/app/

# 类型检查
mypy server/app/
```

3. **运行测试**
```bash
cd server
pytest tests/ -v
pytest tests/ --cov=app --cov-report=html
```

### API 开发流程

#### 1. 创建数据模型 (models/)

```python
# server/app/models/example.py
from sqlalchemy import Column, Integer, String
from .base import BaseModel

class Example(BaseModel):
    __tablename__ = "examples"
    
    name = Column(String(255), nullable=False)
    description = Column(String(500))
```

#### 2. 创建数据模式 (schemas/)

```python
# server/app/schemas/example.py
from pydantic import BaseModel

class ExampleCreate(BaseModel):
    name: str
    description: str | None = None

class ExampleResponse(BaseModel):
    id: int
    name: str
    description: str | None
    
    class Config:
        from_attributes = True
```

#### 3. 创建 API 路由 (api/routers/)

```python
# server/app/api/routers/examples.py
from fastapi import APIRouter, Depends
from sqlalchemy.orm import Session
from ...core.database import get_db
from ...models.example import Example
from ...schemas.example import ExampleCreate, ExampleResponse

router = APIRouter(prefix="/examples", tags=["Examples"])

@router.get("/", response_model=list[ExampleResponse])
async def get_examples(db: Session = Depends(get_db)):
    return db.query(Example).all()

@router.post("/", response_model=ExampleResponse)
async def create_example(
    data: ExampleCreate,
    db: Session = Depends(get_db)
):
    example = Example(**data.dict())
    db.add(example)
    db.commit()
    db.refresh(example)
    return example
```

#### 4. 注册路由 (main.py)

```python
# server/app/main.py
from .api.routers import examples

app.include_router(examples.router, prefix="/api")
```

### 代码规范

#### 命名规范
```python
# 类名: PascalCase
class UserService:
    pass

# 函数名: snake_case
def get_user_by_id(user_id: int):
    pass

# 常量: UPPER_SNAKE_CASE
MAX_RETRY_TIMES = 3

# 私有变量: _开头
_internal_cache = {}
```

#### 文档字符串
```python
def calculate_profit(
    prev_value: float,
    curr_value: float,
    ratio: float
) -> float:
    """
    计算收益分配
    
    Args:
        prev_value: 前一时刻价值
        curr_value: 当前时刻价值
        ratio: 分配比例 (0-10000)
    
    Returns:
        分配后的收益金额
    
    Raises:
        ValueError: 当比例超出范围时
    """
    if not 0 <= ratio <= 10000:
        raise ValueError("Ratio must be between 0 and 10000")
    
    profit = curr_value - prev_value
    return profit * (ratio / 10000)
```

### Git 工作流

```bash
# 创建功能分支
git checkout -b feature/new-feature

# 提交代码
git add .
git commit -m "feat: add new feature"

# 推送到远程
git push origin feature/new-feature

# 创建 Pull Request
# (在 GitHub/GitLab 上操作)
```

### 提交信息规范

```
feat: 新功能
fix: 修复 bug
docs: 文档更新
style: 代码格式调整
refactor: 代码重构
test: 测试相关
chore: 构建/工具相关

示例:
feat: add profit reallocation API
fix: resolve None error in allocation ratios
docs: update API documentation
```

---

## 🔒 安全说明

### 认证机制

- **JWT Token** 认证
- **Token 有效期**: 30 分钟
- **加密算法**: HS256
- **密码加密**: bcrypt (cost factor: 12)

### 权限系统

```python
# 权限格式
"resource:action"

# 示例权限
"user:read"      # 读取用户
"user:write"     # 创建/更新用户
"profit:read"    # 读取收益
"profit:write"   # 修改收益
"admin:all"      # 管理员所有权限
```

### 超级管理员

- `is_super = True` 的用户拥有所有权限
- 默认账户: `admin@example.com`
- ⚠️ **生产环境必须修改默认密码**

### 安全最佳实践

1. **环境变量保护**
   - ✅ 不要提交 `.env` 到 Git
   - ✅ 使用强密码和密钥（32+字符）
   - ✅ 定期轮换 API Keys

2. **API 安全**
   - ✅ 所有敏感端点需要认证
   - ✅ 实施速率限制（未来）
   - ✅ 输入验证和清洗

3. **数据库安全**
   - ✅ 使用 ORM 防止 SQL 注入
   - ✅ 最小权限原则
   - ✅ 定期备份

4. **HTTPS**
   - ⚠️ 生产环境应启用 HTTPS
   - 📝 配置 SSL 证书（Let's Encrypt）

---

## ❓ 常见问题

### Q1: 如何重置管理员密码？

```bash
cd server
python -c "
from app.core.database import SessionLocal
from app.models.user import User
from passlib.context import CryptContext

pwd_context = CryptContext(schemes=['bcrypt'], deprecated='auto')
db = SessionLocal()

user = db.query(User).filter(User.email == 'admin@example.com').first()
user.password_hash = pwd_context.hash('new_password')
db.commit()
print('Password reset successfully')
"
```

### Q2: 如何查看服务状态？

```bash
# 检查进程
pgrep -af uvicorn

# 检查端口
netstat -tlnp | grep -E '8000|8001'

# 检查 Nginx
sudo systemctl status nginx

# 查看日志
tail -f /tmp/uvicorn_8000.log
```

### Q3: 数据库连接失败怎么办？

1. 检查数据库服务是否运行
2. 验证 `.env` 中的连接信息
3. 检查网络和防火墙
4. 查看数据库日志

```bash
# 测试数据库连接
mysql -h <host> -P <port> -u <user> -p <database>
```

### Q4: API 返回 403 Forbidden？

- ✅ 检查是否携带 Token: `Authorization: Bearer <token>`
- ✅ Token 是否过期（30分钟有效期）
- ✅ 用户是否有对应权限
- ✅ 用户账户是否被禁用

### Q5: 如何备份数据库？

```bash
# 导出数据库
mysqldump -h cedefi-database-instance.cwwyatalynow.ap-northeast-1.rds.amazonaws.com \
  -P 49123 -u admin -p fund_management > backup_$(date +%Y%m%d).sql

# 恢复数据库
mysql -h <host> -P <port> -u <user> -p fund_management < backup_20251017.sql
```

### Q6: 如何添加新的 API 端点？

参考 [开发指南](#-开发指南) 中的 API 开发流程。

### Q7: 如何启用 HTTPS？

```bash
# 安装 Certbot
sudo apt install certbot python3-certbot-nginx

# 获取证书
sudo certbot --nginx -d your-domain.com

# 自动续期
sudo certbot renew --dry-run
```

---

## 📝 更新日志

### v1.0.0 (2025-10-17)

#### ✨ 新功能
- ✅ 完整的用户认证与权限系统
- ✅ 投资组合管理
- ✅ 自动化收益分配（小时级）
- ✅ 提现与调账管理
- ✅ 数据快照功能
- ✅ OneToken API 集成
- ✅ Ceffu API 集成
- ✅ Swagger/ReDoc 自动文档

#### 🐛 Bug 修复
- ✅ P0: 修复 allocation ratios None 错误
- ✅ P0: 修复 profit model 字段不匹配
- ✅ P1: 修复 profit withdrawals 字段问题
- ✅ P2: 添加 3 个缺失的 API 端点

#### 📚 文档
- ✅ API 完整文档
- ✅ API 快速开始指南
- ✅ 项目架构文档
- ✅ 项目技术文档
- ✅ 完整 README

#### 🚀 部署
- ✅ AWS EC2 + RDS 部署
- ✅ Nginx 反向代理配置
- ✅ 多进程 Uvicorn 配置
- ✅ 系统健康度: 98%
- ✅ API 可用率: 100%

---

## 📞 联系与支持

### 团队信息

- **项目负责人**: Lucien
- **技术支持**: Lucien

### 在线资源

- 📚 **Swagger UI**: http://13.113.11.170/docs
- 📖 **ReDoc**: http://13.113.11.170/redoc
- 📄 **项目文档**: 见 `docs/` 目录
- 🔗 **GitHub**: 同步中

### 问题反馈

- 🐛 Issue: [GitHub Issues URL]
- 💬 讨论: [Discussions URL]

---

## 📄 许可证

[Lucien的开发许可证]

---

## 🙏 致谢

感谢所有为本项目做出贡献的开发者！

- FastAPI 团队
- SQLAlchemy 团队
- Pydantic 团队
- 所有开源贡献者

---

**Made with ❤️ by Lucien Team**

**Last Updated**: 2025-10-17
