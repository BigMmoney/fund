# 📘 Fund Management API - 项目技术文档

**项目名称**: Fund Management API  
**版本**: v1.0  
**最后更新**: 2025-10-17  
**维护团队**:Lucien

---

## 📋 目录

1. [项目概述](#项目概述)
2. [技术栈](#技术栈)
3. [系统架构](#系统架构)
4. [项目结构](#项目结构)
5. [数据库设计](#数据库设计)
6. [部署架构](#部署架构)
7. [开发指南](#开发指南)
8. [运维手册](#运维手册)
9. [性能优化](#性能优化)
10. [安全策略](#安全策略)

---

## 🎯 项目概述

### 项目简介

Fund Management API 是一个基金管理系统的后端API服务，提供收益分配、提现管理、数据统计等核心功能。

### 核心功能

- **用户认证**: JWT Token 认证，基于角色的权限控制
- **收益管理**: 收益分配比例配置、自动分配、历史记录
- **资金流转**: 提现管理、调账操作、审计日志
- **数据统计**: 累计收益、小时收益、多维度统计
- **Portfolio 管理**: 投资组合管理、NAV 计算
- **健康监控**: 系统健康检查、性能监控

### 业务特点

- **多层级分配**: 支持平台、团队、用户三级收益分配
- **实时计算**: 小时级收益统计，实时更新累计数据
- **审计追踪**: 所有关键操作都有详细日志记录
- **高可用**: 多 worker 部署，Nginx 反向代理
- **安全可靠**: JWT 认证、权限控制、SQL 注入防护

---

## 🛠️ 技术栈

### 后端框架

| 技术 | 版本 | 用途 |
|------|------|------|
| **FastAPI** | 0.104.1 | Web 框架 |
| **Uvicorn** | 0.24.0 | ASGI 服务器 |
| **Python** | 3.12 | 编程语言 |

### 数据库

| 技术 | 版本 | 用途 |
|------|------|------|
| **MySQL** | 8.0 | 主数据库 (AWS RDS) |
| **SQLAlchemy** | 2.0.23 | ORM 框架 |
| **PyMySQL** | 1.1.0 | MySQL 驱动 |

### 认证与安全

| 技术 | 版本 | 用途 |
|------|------|------|
| **python-jose** | 3.3.0 | JWT Token 生成/验证 |
| **passlib** | 1.7.4 | 密码加密 (bcrypt) |
| **cryptography** | 41.0.7 | 加密算法 |

### 工具库

| 技术 | 版本 | 用途 |
|------|------|------|
| **Pydantic** | 2.5.0 | 数据验证 |
| **requests** | 2.31.0 | HTTP 客户端 |
| **python-dotenv** | 1.0.0 | 环境变量管理 |
| **loguru** | 0.7.2 | 日志系统 |

### 基础设施

| 技术 | 用途 |
|------|------|
| **AWS EC2** | 应用服务器 (Tokyo) |
| **AWS RDS** | MySQL 数据库 |
| **Nginx** | 反向代理、负载均衡 |
| **systemd** | 进程管理 |

---

## 🏗️ 系统架构

### 整体架构图

```
┌─────────────────┐
│   Frontend      │
│   (Browser)     │
└────────┬────────┘
         │ HTTP
         ↓
┌─────────────────┐
│  Nginx (Port 80)│
│  Reverse Proxy  │
└────────┬────────┘
         │
         ├─────────────────┐
         ↓                 ↓
┌──────────────┐   ┌──────────────┐
│  Uvicorn     │   │  Uvicorn     │
│  Port 8000   │   │  Port 8001   │
│  2 workers   │   │  1 worker    │
└──────┬───────┘   └──────┬───────┘
       │                  │
       └─────────┬────────┘
                 ↓
         ┌──────────────┐
         │   FastAPI    │
         │  Application │
         └──────┬───────┘
                │
                ↓
         ┌──────────────┐
         │   MySQL      │
         │   Database   │
         │  (AWS RDS)   │
         └──────────────┘
```

### 请求流程

```
1. Client → Nginx (80)
2. Nginx → Uvicorn (8000/8001)
3. Uvicorn → FastAPI App
4. FastAPI → Authentication Middleware
5. FastAPI → Route Handler
6. Handler → Database (via SQLAlchemy)
7. Database → Handler
8. Handler → Response Formatter
9. Response → Client
```

### 三层架构

```
┌─────────────────────────────────────┐
│         Presentation Layer          │
│  (API Routes, Request/Response)     │
│  - auth.py, profits.py, etc.        │
└──────────────┬──────────────────────┘
               │
┌──────────────┴──────────────────────┐
│          Business Layer             │
│  (Business Logic, Validation)       │
│  - Services, Dependencies           │
└──────────────┬──────────────────────┘
               │
┌──────────────┴──────────────────────┐
│           Data Layer                │
│  (Models, Database Access)          │
│  - SQLAlchemy Models, Schemas       │
└─────────────────────────────────────┘
```

---

## 📁 项目结构

### 目录树

```
fund_management_api/
├── server/
│   └── app/
│       ├── __init__.py
│       ├── main.py                    # 应用入口
│       ├── config.py                  # 配置管理
│       ├── database.py                # 数据库连接
│       ├── auth.py                    # 认证逻辑
│       ├── responses.py               # 统一响应格式
│       ├── schemas.py                 # Pydantic Schemas
│       │
│       ├── models/                    # SQLAlchemy 模型
│       │   ├── __init__.py
│       │   ├── base.py                # 基础模型
│       │   ├── user.py                # 用户模型
│       │   ├── team.py                # 团队模型
│       │   ├── portfolio.py           # Portfolio 模型
│       │   ├── profit.py              # 收益相关模型
│       │   ├── snapshots.py           # 快照模型
│       │   └── ...
│       │
│       ├── api/                       # API 路由
│       │   ├── __init__.py
│       │   ├── dependencies.py        # 依赖注入
│       │   └── routers/               # 路由模块
│       │       ├── __init__.py
│       │       ├── auth.py            # 认证路由
│       │       ├── profits.py         # 收益路由
│       │       ├── portfolios.py      # Portfolio 路由
│       │       ├── users.py           # 用户路由
│       │       ├── teams.py           # 团队路由
│       │       ├── health.py          # 健康检查
│       │       └── ...
│       │
│       ├── core/                      # 核心功能
│       │   ├── __init__.py
│       │   ├── database.py            # 数据库核心
│       │   └── security.py            # 安全相关
│       │
│       ├── middleware/                # 中间件
│       │   ├── __init__.py
│       │   ├── auth_middleware.py     # 认证中间件
│       │   └── logging_middleware.py  # 日志中间件
│       │
│       ├── services/                  # 业务服务
│       │   ├── __init__.py
│       │   ├── profit_service.py      # 收益服务
│       │   └── ...
│       │
│       └── utils/                     # 工具函数
│           ├── __init__.py
│           └── ...
│
├── venv/                              # Python 虚拟环境
├── requirements.txt                   # 依赖列表
├── .env                               # 环境变量 (不提交到 git)
└── README.md                          # 项目说明
```

### 核心文件说明

#### 1. `main.py` - 应用入口

```python
from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware

app = FastAPI(
    title="Fund Management API",
    version="1.0.0",
    docs_url="/docs",
    redoc_url="/redoc"
)

# CORS 配置
app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)

# 注册路由
from app.api.routers import profits, auth, portfolios
app.include_router(auth.router, tags=["Auth"])
app.include_router(profits.router, prefix="/profit", tags=["Profit"])
app.include_router(portfolios.router, prefix="/portfolios", tags=["Portfolios"])
```

#### 2. `database.py` - 数据库配置

```python
from sqlalchemy import create_engine
from sqlalchemy.ext.declarative import declarative_base
from sqlalchemy.orm import sessionmaker

SQLALCHEMY_DATABASE_URL = "mysql+pymysql://user:pass@host/db"

engine = create_engine(SQLALCHEMY_DATABASE_URL)
SessionLocal = sessionmaker(bind=engine)
Base = declarative_base()

def get_db():
    db = SessionLocal()
    try:
        yield db
    finally:
        db.close()
```

#### 3. `models/profit.py` - 收益模型

```python
from sqlalchemy import Column, Integer, String, Numeric, DateTime, ForeignKey
from sqlalchemy.orm import relationship
from app.models.base import BaseModel

class ProfitAllocationRatio(BaseModel):
    """收益分配比例"""
    __tablename__ = "profit_allocation_ratios"
    
    portfolio_id = Column(Integer, ForeignKey("portfolios.id"))
    version = Column(Integer, default=1)
    to_team_ratio = Column("to_team", Numeric(5, 4))
    to_platform_ratio = Column("to_platform", Numeric(5, 4))
    to_user_ratio = Column("to_user", Numeric(5, 4))
    created_by = Column(Integer, ForeignKey("users.id"))
    
    # 关系
    portfolio = relationship("Portfolio")
    creator = relationship("User")

class ProfitWithdrawal(BaseModel):
    """提现记录"""
    __tablename__ = "profit_withdrawals"
    
    from_type = Column(String(50), nullable=False)
    team_id = Column(Integer, ForeignKey("teams.id"))
    user_id = Column(Integer, ForeignKey("users.id"))
    amount = Column(Numeric(20, 8), nullable=False)
    status = Column(String(20), default="pending")
    
    # 关系
    team = relationship("Team")
    user = relationship("User")
```

#### 4. `api/routers/profits.py` - 收益路由

```python
from fastapi import APIRouter, Depends, Query
from sqlalchemy.orm import Session
from app.database import get_db
from app.api.dependencies import require_profit_permission
from app.responses import StandardResponse

router = APIRouter()

@router.get("/profit_allocation_ratios")
async def get_profit_allocation_ratios(
    limit: int = Query(100, ge=1, le=1000),
    offset: int = Query(0, ge=0),
    db: Session = Depends(get_db),
    current_user = Depends(require_profit_permission)
):
    """获取收益分配比例"""
    query = db.query(ProfitAllocationRatio)
    total = query.count()
    ratios = query.offset(offset).limit(limit).all()
    
    return StandardResponse.list_success(
        [ratio.to_dict() for ratio in ratios],
        total
    )
```

#### 5. `responses.py` - 统一响应格式

```python
from typing import Any, Optional

class StandardResponse:
    @staticmethod
    def success(data: Any, message: str = "Success"):
        return {
            "isOK": True,
            "message": message,
            "data": data
        }
    
    @staticmethod
    def list_success(data: list, total: int):
        return {
            "isOK": True,
            "message": "Success",
            "data": data,
            "total": total
        }
    
    @staticmethod
    def error(message: str, error_code: int = 500):
        return {
            "isOK": False,
            "message": message,
            "data": {"errorCode": error_code}
        }
```

---

## 🗄️ 数据库设计

### ER 图概览

```
┌─────────────┐         ┌──────────────┐
│    Users    │────────<│  Portfolios  │
└─────────────┘         └──────────────┘
       │                       │
       │                       │
       ↓                       ↓
┌─────────────┐         ┌──────────────┐
│    Teams    │         │    Profit    │
└─────────────┘         │  Allocation  │
                        │    Ratios    │
                        └──────────────┘
```

### 核心表结构

#### 1. users - 用户表

```sql
CREATE TABLE users (
    id INT PRIMARY KEY AUTO_INCREMENT,
    email VARCHAR(255) UNIQUE NOT NULL,
    username VARCHAR(255) NOT NULL,
    hashed_password VARCHAR(255) NOT NULL,
    role VARCHAR(50) NOT NULL,
    is_active BOOLEAN DEFAULT TRUE,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME ON UPDATE CURRENT_TIMESTAMP,
    INDEX idx_email (email),
    INDEX idx_role (role)
);
```

**字段说明**:
- `id`: 主键
- `email`: 邮箱（用于登录）
- `hashed_password`: 加密后的密码
- `role`: 角色（admin, user, viewer）
- `is_active`: 是否激活

#### 2. teams - 团队表

```sql
CREATE TABLE teams (
    id INT PRIMARY KEY AUTO_INCREMENT,
    name VARCHAR(255) NOT NULL,
    description TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_name (name)
);
```

#### 3. portfolios - Portfolio 表

```sql
CREATE TABLE portfolios (
    id INT PRIMARY KEY AUTO_INCREMENT,
    name VARCHAR(255) NOT NULL,
    description TEXT,
    user_id INT,
    team_id INT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME ON UPDATE CURRENT_TIMESTAMP,
    FOREIGN KEY (user_id) REFERENCES users(id),
    FOREIGN KEY (team_id) REFERENCES teams(id),
    INDEX idx_user_id (user_id),
    INDEX idx_team_id (team_id)
);
```

#### 4. profit_allocation_ratios - 收益分配比例表

```sql
CREATE TABLE profit_allocation_ratios (
    id INT PRIMARY KEY AUTO_INCREMENT,
    portfolio_id INT NOT NULL,
    version INT DEFAULT 1,
    to_team DECIMAL(5,4) NOT NULL COMMENT '分配给团队的比例',
    to_platform DECIMAL(5,4) NOT NULL COMMENT '分配给平台的比例',
    to_user DECIMAL(5,4) NOT NULL COMMENT '分配给用户的比例',
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    created_by INT,
    FOREIGN KEY (portfolio_id) REFERENCES portfolios(id),
    FOREIGN KEY (created_by) REFERENCES users(id),
    INDEX idx_portfolio_id (portfolio_id),
    INDEX idx_version (version)
);
```

**约束**: `to_team + to_platform + to_user = 1.0`

#### 5. profit_withdrawals - 提现记录表

```sql
CREATE TABLE profit_withdrawals (
    id INT PRIMARY KEY AUTO_INCREMENT,
    from_type VARCHAR(50) NOT NULL COMMENT 'user/team/platform',
    team_id INT,
    user_id INT,
    amount DECIMAL(20,8) NOT NULL,
    status VARCHAR(20) DEFAULT 'pending' COMMENT 'pending/completed/failed',
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    completed_at DATETIME,
    FOREIGN KEY (team_id) REFERENCES teams(id),
    FOREIGN KEY (user_id) REFERENCES users(id),
    INDEX idx_from_type (from_type),
    INDEX idx_status (status),
    INDEX idx_created_at (created_at)
);
```

#### 6. profit_reallocations - 调账记录表

```sql
CREATE TABLE profit_reallocations (
    id INT PRIMARY KEY AUTO_INCREMENT,
    from_type VARCHAR(50) NOT NULL,
    from_team_id INT,
    to_type VARCHAR(50) NOT NULL,
    to_team_id INT,
    user_id INT,
    amount DECIMAL(20,8) NOT NULL,
    reason TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    created_by INT,
    FOREIGN KEY (from_team_id) REFERENCES teams(id),
    FOREIGN KEY (to_team_id) REFERENCES teams(id),
    FOREIGN KEY (user_id) REFERENCES users(id),
    FOREIGN KEY (created_by) REFERENCES users(id),
    INDEX idx_created_at (created_at)
);
```

#### 7. profit_allocation_logs - 分配日志表

```sql
CREATE TABLE profit_allocation_logs (
    id INT PRIMARY KEY AUTO_INCREMENT,
    portfolio_id INT NOT NULL,
    profit_amount DECIMAL(20,8) NOT NULL,
    to_team_amount DECIMAL(20,8) NOT NULL,
    to_platform_amount DECIMAL(20,8) NOT NULL,
    to_user_amount DECIMAL(20,8) NOT NULL,
    allocation_ratio_id INT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (portfolio_id) REFERENCES portfolios(id),
    FOREIGN KEY (allocation_ratio_id) REFERENCES profit_allocation_ratios(id),
    INDEX idx_portfolio_id (portfolio_id),
    INDEX idx_created_at (created_at)
);
```

#### 8. acc_profit_from_portfolio - 累计收益表

```sql
CREATE TABLE acc_profit_from_portfolio (
    id INT PRIMARY KEY AUTO_INCREMENT,
    portfolio_id INT NOT NULL UNIQUE,
    acc_profit DECIMAL(20,8) DEFAULT 0,
    updated_at DATETIME ON UPDATE CURRENT_TIMESTAMP,
    FOREIGN KEY (portfolio_id) REFERENCES portfolios(id),
    INDEX idx_updated_at (updated_at)
);
```

#### 9. hourly_profit_user - 用户小时收益表

```sql
CREATE TABLE hourly_profit_user (
    id INT PRIMARY KEY AUTO_INCREMENT,
    user_id INT NOT NULL,
    hour_time DATETIME NOT NULL,
    hour_profit DECIMAL(20,8) NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (user_id) REFERENCES users(id),
    UNIQUE KEY uk_user_hour (user_id, hour_time),
    INDEX idx_hour_time (hour_time)
);
```

### 索引策略

| 表名 | 索引类型 | 索引字段 | 用途 |
|------|---------|---------|------|
| users | UNIQUE | email | 登录查询 |
| users | INDEX | role | 权限查询 |
| portfolios | INDEX | user_id | 用户 Portfolio 查询 |
| profit_allocation_logs | INDEX | created_at | 时间范围查询 |
| hourly_profit_user | UNIQUE | (user_id, hour_time) | 防重复插入 |

---

## 🚀 部署架构

### 服务器信息

```
服务器: AWS EC2
实例类型: [根据实际情况]
区域: ap-northeast-1 (Tokyo)
操作系统: Ubuntu 22.04 LTS
IP: 13.113.11.170
域名: (可选配置)
```

### 服务部署

#### Nginx 配置

**文件位置**: `/etc/nginx/sites-available/fund-api`

```nginx
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
        
        # WebSocket 支持
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        
        # 超时配置
        proxy_connect_timeout 60s;
        proxy_send_timeout 60s;
        proxy_read_timeout 60s;
    }

    # 静态文件缓存
    location ~* \.(jpg|jpeg|png|gif|ico|css|js)$ {
        expires 7d;
    }
}
```

#### Uvicorn 启动

**端口 8000** (2 workers):
```bash
cd /home/ubuntu/fund_management_api/server
nohup /home/ubuntu/fund_management_api/venv/bin/python -m uvicorn app.main:app \
  --host 0.0.0.0 \
  --port 8000 \
  --workers 2 \
  > /tmp/uvicorn_8000.log 2>&1 &
```

**端口 8001** (1 worker):
```bash
cd /home/ubuntu/fund_management_api/server
nohup /home/ubuntu/fund_management_api/venv/bin/python -m uvicorn app.main:app \
  --host 0.0.0.0 \
  --port 8001 \
  --workers 1 \
  > /tmp/uvicorn_8001.log 2>&1 &
```

### 数据库配置

```
数据库: AWS RDS MySQL
引擎版本: 8.0
实例类型: [根据实际情况]
存储: [根据实际情况] GB
连接池: 10-20 连接
```

**连接配置**:
```python
SQLALCHEMY_DATABASE_URL = (
    f"mysql+pymysql://{USER}:{PASSWORD}@{HOST}:{PORT}/{DATABASE}"
    "?charset=utf8mb4"
)

engine = create_engine(
    SQLALCHEMY_DATABASE_URL,
    pool_size=10,
    max_overflow=20,
    pool_pre_ping=True,
    pool_recycle=3600
)
```

### 环境变量

**`.env` 文件**:
```bash
# Database
DATABASE_URL=mysql+pymysql://user:pass@host/db

# JWT
SECRET_KEY=your-secret-key-here
ALGORITHM=HS256
ACCESS_TOKEN_EXPIRE_MINUTES=1440

# Application
DEBUG=false
ALLOWED_HOSTS=*
```

---

## 💻 开发指南

### 本地开发环境搭建

#### 1. 克隆项目

```bash
git clone <repository-url>
cd fund_management_api
```

#### 2. 创建虚拟环境

```bash
python3 -m venv venv
source venv/bin/activate  # Linux/Mac
# 或
venv\Scripts\activate     # Windows
```

#### 3. 安装依赖

```bash
pip install -r requirements.txt
```

#### 4. 配置环境变量

```bash
cp .env.example .env
# 编辑 .env 文件，配置数据库连接等
```

#### 5. 运行开发服务器

```bash
cd server
uvicorn app.main:app --reload --port 8000
```

#### 6. 访问文档

```
http://localhost:8000/docs
```

### 代码规范

#### Python 代码风格

遵循 PEP 8 规范：

```python
# 导入顺序
import os  # 标准库
import sys

from fastapi import FastAPI  # 第三方库
from sqlalchemy import Column

from app.models import User  # 本地模块

# 命名规范
class UserService:  # 类名: PascalCase
    def get_user(self):  # 函数名: snake_case
        user_name = "John"  # 变量名: snake_case
        MAX_RETRY = 3  # 常量: UPPER_CASE
```

#### 注释规范

```python
def calculate_profit(
    amount: float,
    ratio: float
) -> float:
    """
    计算收益金额
    
    Args:
        amount: 总金额
        ratio: 分配比例 (0-1)
    
    Returns:
        计算后的收益金额
    
    Raises:
        ValueError: 如果 ratio 不在 0-1 范围内
    """
    if not 0 <= ratio <= 1:
        raise ValueError("Ratio must be between 0 and 1")
    
    return amount * ratio
```

### API 开发流程

#### 1. 定义数据模型

```python
# models/new_feature.py
from app.models.base import BaseModel
from sqlalchemy import Column, Integer, String

class NewFeature(BaseModel):
    __tablename__ = "new_features"
    
    name = Column(String(255), nullable=False)
    description = Column(String(500))
```

#### 2. 创建 Pydantic Schema

```python
# schemas.py
from pydantic import BaseModel

class NewFeatureCreate(BaseModel):
    name: str
    description: str | None = None

class NewFeatureResponse(BaseModel):
    id: int
    name: str
    description: str | None
```

#### 3. 实现路由

```python
# api/routers/new_feature.py
from fastapi import APIRouter, Depends
from sqlalchemy.orm import Session

router = APIRouter()

@router.post("/new-features")
async def create_feature(
    data: NewFeatureCreate,
    db: Session = Depends(get_db)
):
    feature = NewFeature(**data.dict())
    db.add(feature)
    db.commit()
    return StandardResponse.success(feature.to_dict())
```

#### 4. 注册路由

```python
# main.py
from app.api.routers import new_feature

app.include_router(
    new_feature.router,
    prefix="/api/v1",
    tags=["New Feature"]
)
```

### 测试

#### 单元测试

```python
# tests/test_profits.py
import pytest
from fastapi.testclient import TestClient
from app.main import app

client = TestClient(app)

def test_get_profit_ratios():
    response = client.get("/profit/profit_allocation_ratios?limit=10")
    assert response.status_code == 200
    data = response.json()
    assert data["isOK"] == True
    assert "data" in data
```

#### 运行测试

```bash
pytest tests/ -v
```

---

## 🔧 运维手册

### 日常运维

#### 1. 查看服务状态

```bash
# 查看 Uvicorn 进程
ps aux | grep uvicorn

# 查看端口监听
netstat -tlnp | grep -E '8000|8001'

# 查看 Nginx 状态
sudo systemctl status nginx
```

#### 2. 查看日志

```bash
# Uvicorn 日志
tail -f /tmp/uvicorn_8000.log
tail -f /tmp/uvicorn_8001.log

# Nginx 日志
sudo tail -f /var/log/nginx/access.log
sudo tail -f /var/log/nginx/error.log
```

#### 3. 重启服务

```bash
# 重启 Uvicorn
pkill -f "uvicorn.*8000"
pkill -f "uvicorn.*8001"

# 启动服务
cd /home/ubuntu/fund_management_api/server
nohup ../venv/bin/python -m uvicorn app.main:app --host 0.0.0.0 --port 8000 --workers 2 &
nohup ../venv/bin/python -m uvicorn app.main:app --host 0.0.0.0 --port 8001 --workers 1 &

# 重启 Nginx
sudo systemctl restart nginx
```

#### 4. 更新代码

```bash
# 1. 拉取最新代码
cd /home/ubuntu/fund_management_api
git pull origin main

# 2. 安装新依赖（如果有）
source venv/bin/activate
pip install -r requirements.txt

# 3. 重启服务
pkill -f uvicorn
# 然后启动服务...
```

### 备份策略

#### 数据库备份

```bash
# 每日自动备份
0 2 * * * mysqldump -h RDS_HOST -u USER -p'PASSWORD' DATABASE > /backup/db_$(date +\%Y\%m\%d).sql
```

#### 代码备份

```bash
# 使用 Git 版本控制
git add .
git commit -m "Update"
git push origin main
```

### 监控

#### 健康检查

```bash
# API 健康检查
curl http://localhost/health

# 数据库连接检查
mysql -h RDS_HOST -u USER -p'PASSWORD' -e "SELECT 1"
```

#### 性能监控

```bash
# CPU 使用率
top -b -n 1 | grep uvicorn

# 内存使用率
ps aux | grep uvicorn | awk '{print $4, $6}'

# 磁盘空间
df -h
```

---

## ⚡ 性能优化

### 数据库优化

#### 1. 索引优化

```sql
-- 查询慢查询
SELECT * FROM mysql.slow_log WHERE query_time > 1;

-- 添加复合索引
CREATE INDEX idx_user_time ON hourly_profit_user(user_id, hour_time);
```

#### 2. 查询优化

```python
# 使用 joinedload 减少 N+1 查询
from sqlalchemy.orm import joinedload

query = db.query(ProfitWithdrawal).options(
    joinedload(ProfitWithdrawal.user),
    joinedload(ProfitWithdrawal.team)
)
```

#### 3. 连接池配置

```python
engine = create_engine(
    DATABASE_URL,
    pool_size=20,        # 增加连接池大小
    max_overflow=40,     # 增加最大溢出连接
    pool_pre_ping=True,  # 连接前检查
    pool_recycle=3600    # 1小时回收连接
)
```

### 应用优化

#### 1. 缓存

```python
from functools import lru_cache

@lru_cache(maxsize=128)
def get_profit_ratio(portfolio_id: int):
    # 缓存配置数据
    return db.query(ProfitAllocationRatio).filter_by(
        portfolio_id=portfolio_id
    ).first()
```

#### 2. 异步处理

```python
from fastapi import BackgroundTasks

@router.post("/profit/allocate")
async def allocate_profit(
    data: dict,
    background_tasks: BackgroundTasks
):
    # 立即返回
    background_tasks.add_task(process_allocation, data)
    return {"status": "processing"}
```

#### 3. 分页优化

```python
# 使用游标分页而非偏移分页
@router.get("/profits")
async def get_profits(cursor: int = None, limit: int = 100):
    query = db.query(Profit)
    if cursor:
        query = query.filter(Profit.id > cursor)
    return query.limit(limit).all()
```

---

## 🔒 安全策略

### 认证安全

#### 1. 密码加密

```python
from passlib.context import CryptContext

pwd_context = CryptContext(schemes=["bcrypt"], deprecated="auto")

# 加密密码
hashed = pwd_context.hash("password")

# 验证密码
is_valid = pwd_context.verify("password", hashed)
```

#### 2. JWT Token

```python
from jose import jwt
from datetime import datetime, timedelta

def create_token(data: dict, expires_delta: timedelta = None):
    to_encode = data.copy()
    expire = datetime.utcnow() + (expires_delta or timedelta(minutes=15))
    to_encode.update({"exp": expire})
    return jwt.encode(to_encode, SECRET_KEY, algorithm=ALGORITHM)
```

### 输入验证

```python
from pydantic import validator, EmailStr

class UserCreate(BaseModel):
    email: EmailStr
    password: str
    
    @validator('password')
    def password_strength(cls, v):
        if len(v) < 8:
            raise ValueError('Password must be at least 8 characters')
        return v
```

### SQL 注入防护

```python
# ✅ 正确：使用参数化查询
query = db.query(User).filter(User.email == email)

# ❌ 错误：直接拼接 SQL
query = f"SELECT * FROM users WHERE email = '{email}'"
```

### CORS 配置

```python
from fastapi.middleware.cors import CORSMiddleware

app.add_middleware(
    CORSMiddleware,
    allow_origins=["https://yourdomain.com"],  # 生产环境限制域名
    allow_credentials=True,
    allow_methods=["GET", "POST", "PUT", "DELETE"],
    allow_headers=["*"],
)
```

---

## 📞 联系与支持

**技术负责人**: Lucien
**文档更新**: 2025-10-17  
**项目状态**: ✅ 生产环境运行中

---

**文档结束** 📚
