# 📚 Fund Management API - 完整文档

**版本**: v1.0  
**最后更新**: 2025-10-17  
**服务器**: http://13.113.11.170  
**状态**: ✅ 生产就绪

---

## 📋 目录

1. [快速开始](#快速开始)
2. [API 基础信息](#api-基础信息)
3. [认证授权](#认证授权)
4. [核心 API 端点](#核心-api-端点)
5. [数据模型](#数据模型)
6. [错误处理](#错误处理)
7. [代码示例](#代码示例)
8. [最佳实践](#最佳实践)

---

## 🚀 快速开始

### 1. 访问 API 文档

**Swagger UI** (推荐 - 可交互测试):
```
http://13.113.11.170/docs
```

**ReDoc** (更适合阅读):
```
http://13.113.11.170/redoc
```

**OpenAPI 规范**:
```
http://13.113.11.170/openapi.json
```

### 2. 基础信息

```
Base URL:     http://13.113.11.170
API 版本:     v1.0
协议:         HTTP/HTTPS
数据格式:     JSON
字符编码:     UTF-8
时区:         UTC
```

### 3. 第一个请求

```bash
# 1. 登录获取 Token
curl -X POST http://13.113.11.170/auth/login \
  -H "Content-Type: application/json" \
  -d '{"email":"user@example.com","password":"your_password"}'

# 2. 使用 Token 访问 API
curl http://13.113.11.170/profit/profit_allocation_ratios?limit=10 \
  -H "Authorization: Bearer YOUR_TOKEN_HERE"
```

---

## 🌐 API 基础信息

### URL 结构

```
http://13.113.11.170/{endpoint}?{params}
```

**示例**:
```
http://13.113.11.170/profit/profit_allocation_ratios?limit=10&offset=0
```

### HTTP 方法

| 方法 | 用途 | 示例 |
|------|------|------|
| GET | 获取资源 | 获取收益列表 |
| POST | 创建资源 | 创建新的分配比例 |
| PUT | 更新资源 | 更新分配比例 |
| DELETE | 删除资源 | 删除记录 |

### 请求头

**必需**:
```
Content-Type: application/json
```

**需要认证的端点**:
```
Authorization: Bearer {token}
```

**可选**:
```
Accept: application/json
User-Agent: YourApp/1.0
```

### 响应格式

所有 API 使用统一的响应格式：

**成功响应**:
```json
{
  "isOK": true,
  "message": "Success",
  "data": {
    // 实际数据
  },
  "total": 100  // 列表端点会有此字段
}
```

**错误响应**:
```json
{
  "isOK": false,
  "message": "Error description",
  "data": {
    "errorCode": 403,
    "detail": "Not authenticated"
  }
}
```

---

## 🔐 认证授权

### 认证流程

1. **用户登录** → 获取 JWT Token
2. **携带 Token** → 访问受保护的 API
3. **Token 过期** → 重新登录获取新 Token

### 登录 API

**端点**: `POST /auth/login`

**请求**:
```json
{
  "email": "user@example.com",
  "password": "your_password"
}
```

**响应**:
```json
{
  "isOK": true,
  "message": "Login successful",
  "data": {
    "token": "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxIiwiZXhwIjoxNjk...",
    "user": {
      "id": 1,
      "email": "user@example.com",
      "username": "John Doe",
      "role": "admin"
    }
  }
}
```

### 使用 Token

在所有需要认证的请求中添加 Header：

```
Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9...
```

### Token 说明

- **类型**: JWT (JSON Web Token)
- **位置**: HTTP Header
- **前缀**: "Bearer "
- **有效期**: [根据服务器配置]
- **刷新**: Token 过期后需要重新登录

### 权限系统

不同的 API 端点需要不同的权限：

| 权限类型 | 说明 | 适用端点 |
|---------|------|---------|
| `public` | 无需认证 | `/health`, `/docs` |
| `authenticated` | 需要登录 | 大部分端点 |
| `profit_permission` | 需要收益权限 | `/profit/*` |
| `admin` | 需要管理员权限 | 用户管理等 |

---

## 📊 核心 API 端点

### 1. Profit Management (收益管理)

#### 1.1 获取收益分配比例

获取 Portfolio 的收益分配参数配置。

**端点**: `GET /profit/profit_allocation_ratios`

**权限**: 需要 `profit_permission`

**参数**:
| 参数 | 类型 | 必需 | 说明 |
|------|------|------|------|
| limit | int | 否 | 每页数量 (1-1000, 默认100) |
| offset | int | 否 | 偏移量 (默认0) |
| portfolio_id | int[] | 否 | Portfolio ID 过滤 |

**请求示例**:
```bash
GET /profit/profit_allocation_ratios?limit=10&offset=0&portfolio_id=1&portfolio_id=2
```

**响应示例**:
```json
{
  "isOK": true,
  "message": "Success",
  "data": [
    {
      "id": 1,
      "portfolioId": 1,
      "version": 1,
      "toTeamRatio": 0.3,
      "toPlatformRatio": 0.1,
      "toUserRatio": 0.6,
      "createdAt": 1697500800,
      "createdBy": 1
    }
  ],
  "total": 1
}
```

**字段说明**:
- `toTeamRatio`: 分配给团队的比例 (0-1)
- `toPlatformRatio`: 分配给平台的比例 (0-1)
- `toUserRatio`: 分配给用户的比例 (0-1)
- 三者之和应该等于 1

---

#### 1.2 获取提现记录

获取虚拟账户的提现记录。

**端点**: `GET /profit/profit_withdrawals`

**权限**: 需要 `profit_permission`

**参数**:
| 参数 | 类型 | 必需 | 说明 |
|------|------|------|------|
| limit | int | 否 | 每页数量 (默认100) |
| offset | int | 否 | 偏移量 (默认0) |

**响应示例**:
```json
{
  "isOK": true,
  "message": "Success",
  "data": [
    {
      "id": 1,
      "fromType": "user",
      "teamId": 1,
      "userId": 1,
      "amount": 1000.50,
      "status": "completed",
      "createdAt": 1697500800,
      "completedAt": 1697501000
    }
  ],
  "total": 1
}
```

**字段说明**:
- `fromType`: 提现来源类型 (user/team/platform)
- `amount`: 提现金额
- `status`: 状态 (pending/completed/failed)

---

#### 1.3 获取调账记录

获取收益调账记录。

**端点**: `GET /profit/profit_reallocations`

**权限**: 需要 `profit_permission`

**参数**:
| 参数 | 类型 | 必需 | 说明 |
|------|------|------|------|
| limit | int | 否 | 每页数量 (默认100) |
| offset | int | 否 | 偏移量 (默认0) |

**响应示例**:
```json
{
  "isOK": true,
  "message": "Success",
  "data": [
    {
      "id": 1,
      "fromType": "team",
      "fromTeamId": 1,
      "toType": "user",
      "toTeamId": null,
      "userId": 5,
      "amount": 500.00,
      "reason": "Bonus allocation",
      "createdAt": 1697500800,
      "createdBy": 1
    }
  ],
  "total": 1
}
```

**字段说明**:
- `fromType`/`toType`: 来源/目标类型 (user/team/platform)
- `amount`: 调账金额
- `reason`: 调账原因

---

#### 1.4 获取收益分配日志

获取每次收益分配的详细记录。

**端点**: `GET /profit/profit_allocation_logs`

**权限**: 需要 `profit_permission`

**参数**:
| 参数 | 类型 | 必需 | 说明 |
|------|------|------|------|
| limit | int | 否 | 每页数量 (默认100) |
| offset | int | 否 | 偏移量 (默认0) |
| portfolio_id | int | 否 | Portfolio ID 过滤 |

**响应示例**:
```json
{
  "isOK": true,
  "message": "Success",
  "data": [
    {
      "id": 1,
      "portfolioId": 1,
      "profitAmount": 10000.00,
      "toTeamAmount": 3000.00,
      "toPlatformAmount": 1000.00,
      "toUserAmount": 6000.00,
      "allocationRatioId": 1,
      "createdAt": 1697500800
    }
  ],
  "total": 1
}
```

**用途**: 审计追踪，查看历史分配记录

---

#### 1.5 获取 Portfolio 累计收益

获取每个 Portfolio 的累计收益数据。

**端点**: `GET /profit/acc_profit_from_portfolio`

**权限**: 需要 `profit_permission`

**参数**:
| 参数 | 类型 | 必需 | 说明 |
|------|------|------|------|
| limit | int | 否 | 每页数量 (默认100) |
| offset | int | 否 | 偏移量 (默认0) |
| portfolio_id | int | 否 | Portfolio ID 过滤 |

**响应示例**:
```json
{
  "isOK": true,
  "message": "Success",
  "data": [
    {
      "id": 1,
      "portfolioId": 1,
      "accProfit": 125000.50,
      "updatedAt": 1697500800
    }
  ],
  "total": 1
}
```

**用途**: 投资回报分析，Portfolio 总收益统计

---

#### 1.6 获取用户小时收益

获取用户的小时级收益数据。

**端点**: `GET /profit/hourly_profit_user`

**权限**: 需要 `profit_permission`

**参数**:
| 参数 | 类型 | 必需 | 说明 |
|------|------|------|------|
| limit | int | 否 | 每页数量 (默认100) |
| offset | int | 否 | 偏移量 (默认0) |
| user_id | int | 否 | 用户 ID 过滤 |
| start_time | int | 否 | 开始时间戳 (Unix timestamp) |
| end_time | int | 否 | 结束时间戳 (Unix timestamp) |

**响应示例**:
```json
{
  "isOK": true,
  "message": "Success",
  "data": [
    {
      "id": 1,
      "userId": 5,
      "hourTime": 1697500800,
      "hourProfit": 125.50,
      "createdAt": 1697500900
    }
  ],
  "total": 24
}
```

**用途**: 用户收益趋势分析，按小时统计

---

### 2. 其他核心端点

#### 2.1 健康检查

**端点**: `GET /health`

**权限**: 无需认证

**响应**:
```json
{
  "status": "healthy",
  "timestamp": 1697500800,
  "version": "1.0.0"
}
```

#### 2.2 获取用户信息

**端点**: `GET /users/me`

**权限**: 需要认证

**响应**:
```json
{
  "isOK": true,
  "data": {
    "id": 1,
    "email": "user@example.com",
    "username": "John Doe",
    "role": "admin",
    "createdAt": 1697500800
  }
}
```

---

## 📐 数据模型

### ProfitAllocationRatio (收益分配比例)

```typescript
interface ProfitAllocationRatio {
  id: number;
  portfolioId: number;
  version: number;
  toTeamRatio: number;      // 0-1 之间
  toPlatformRatio: number;  // 0-1 之间
  toUserRatio: number;      // 0-1 之间
  createdAt: number;        // Unix timestamp
  createdBy: number;
}
```

### ProfitWithdrawal (提现记录)

```typescript
interface ProfitWithdrawal {
  id: number;
  fromType: "user" | "team" | "platform";
  teamId: number | null;
  userId: number | null;
  amount: number;
  status: "pending" | "completed" | "failed";
  createdAt: number;
  completedAt: number | null;
}
```

### ProfitReallocation (调账记录)

```typescript
interface ProfitReallocation {
  id: number;
  fromType: "user" | "team" | "platform";
  fromTeamId: number | null;
  toType: "user" | "team" | "platform";
  toTeamId: number | null;
  userId: number | null;
  amount: number;
  reason: string;
  createdAt: number;
  createdBy: number;
}
```

### ProfitAllocationLog (分配日志)

```typescript
interface ProfitAllocationLog {
  id: number;
  portfolioId: number;
  profitAmount: number;
  toTeamAmount: number;
  toPlatformAmount: number;
  toUserAmount: number;
  allocationRatioId: number;
  createdAt: number;
}
```

### AccProfitFromPortfolio (累计收益)

```typescript
interface AccProfitFromPortfolio {
  id: number;
  portfolioId: number;
  accProfit: number;
  updatedAt: number;
}
```

### HourlyProfitUser (小时收益)

```typescript
interface HourlyProfitUser {
  id: number;
  userId: number;
  hourTime: number;      // Unix timestamp
  hourProfit: number;
  createdAt: number;
}
```

---

## ⚠️ 错误处理

### HTTP 状态码

| 状态码 | 含义 | 说明 |
|--------|------|------|
| 200 | OK | 请求成功 |
| 201 | Created | 资源创建成功 |
| 400 | Bad Request | 请求参数错误 |
| 401 | Unauthorized | 未认证 |
| 403 | Forbidden | 无权限 |
| 404 | Not Found | 资源不存在 |
| 422 | Unprocessable Entity | 参数验证失败 |
| 429 | Too Many Requests | 请求过于频繁 |
| 500 | Internal Server Error | 服务器错误 |

### 错误响应格式

```json
{
  "isOK": false,
  "message": "Error description",
  "data": {
    "errorCode": 422,
    "detail": [
      {
        "loc": ["query", "limit"],
        "msg": "ensure this value is less than or equal to 1000",
        "type": "value_error.number.not_le"
      }
    ]
  }
}
```

### 常见错误处理

#### 1. 认证错误 (403)

```json
{
  "isOK": false,
  "message": "Not authenticated",
  "data": {
    "errorCode": 403
  }
}
```

**解决**: 检查 Authorization Header 是否正确

#### 2. 参数验证错误 (422)

```json
{
  "isOK": false,
  "message": "Validation error",
  "data": {
    "errorCode": 422,
    "detail": [...]
  }
}
```

**解决**: 检查请求参数类型和范围

#### 3. 资源不存在 (404)

```json
{
  "isOK": false,
  "message": "Not Found",
  "data": {
    "errorCode": 404
  }
}
```

**解决**: 检查 URL 和资源 ID

---

## 💻 代码示例

### JavaScript/TypeScript

#### 使用 Fetch API

```typescript
// 配置
const API_BASE = 'http://13.113.11.170';

// 1. 登录
async function login(email: string, password: string): Promise<string> {
  const response = await fetch(`${API_BASE}/auth/login`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ email, password })
  });
  
  if (!response.ok) {
    throw new Error(`Login failed: ${response.status}`);
  }
  
  const data = await response.json();
  return data.data.token;
}

// 2. 创建 API 客户端
class ApiClient {
  private token: string = '';
  
  setToken(token: string) {
    this.token = token;
  }
  
  async request(endpoint: string, options: RequestInit = {}) {
    const headers = {
      'Content-Type': 'application/json',
      ...(this.token && { 'Authorization': `Bearer ${this.token}` }),
      ...options.headers
    };
    
    const response = await fetch(`${API_BASE}${endpoint}`, {
      ...options,
      headers
    });
    
    if (!response.ok) {
      const error = await response.json();
      throw new Error(error.message || 'Request failed');
    }
    
    return response.json();
  }
  
  // 获取收益分配比例
  async getProfitRatios(params: { limit?: number; offset?: number; portfolio_id?: number[] }) {
    const query = new URLSearchParams();
    if (params.limit) query.append('limit', params.limit.toString());
    if (params.offset) query.append('offset', params.offset.toString());
    if (params.portfolio_id) {
      params.portfolio_id.forEach(id => query.append('portfolio_id', id.toString()));
    }
    
    return this.request(`/profit/profit_allocation_ratios?${query}`);
  }
  
  // 获取提现记录
  async getWithdrawals(limit = 100, offset = 0) {
    return this.request(`/profit/profit_withdrawals?limit=${limit}&offset=${offset}`);
  }
  
  // 获取用户小时收益
  async getHourlyProfit(userId: number, startTime?: number, endTime?: number) {
    const query = new URLSearchParams({ user_id: userId.toString() });
    if (startTime) query.append('start_time', startTime.toString());
    if (endTime) query.append('end_time', endTime.toString());
    
    return this.request(`/profit/hourly_profit_user?${query}`);
  }
}

// 使用示例
const api = new ApiClient();

async function main() {
  // 登录
  const token = await login('user@example.com', 'password123');
  api.setToken(token);
  
  // 获取数据
  const ratios = await api.getProfitRatios({ limit: 10 });
  console.log('Profit Ratios:', ratios.data);
  
  const withdrawals = await api.getWithdrawals(10, 0);
  console.log('Withdrawals:', withdrawals.data);
}
```

#### 使用 Axios

```typescript
import axios, { AxiosInstance } from 'axios';

class ApiService {
  private client: AxiosInstance;
  
  constructor() {
    this.client = axios.create({
      baseURL: 'http://13.113.11.170',
      headers: {
        'Content-Type': 'application/json'
      }
    });
    
    // 响应拦截器 - 统一错误处理
    this.client.interceptors.response.use(
      response => response,
      error => {
        if (error.response?.status === 403) {
          // Token 过期，跳转到登录页
          window.location.href = '/login';
        }
        return Promise.reject(error);
      }
    );
  }
  
  setToken(token: string) {
    this.client.defaults.headers.common['Authorization'] = `Bearer ${token}`;
  }
  
  async login(email: string, password: string) {
    const { data } = await this.client.post('/auth/login', { email, password });
    this.setToken(data.data.token);
    return data.data;
  }
  
  async getProfitRatios(params: any) {
    const { data } = await this.client.get('/profit/profit_allocation_ratios', { params });
    return data.data;
  }
  
  async getWithdrawals(limit = 100, offset = 0) {
    const { data } = await this.client.get('/profit/profit_withdrawals', {
      params: { limit, offset }
    });
    return data.data;
  }
}

// 使用
const api = new ApiService();
await api.login('user@example.com', 'password123');
const ratios = await api.getProfitRatios({ limit: 10 });
```

### React Hooks

```typescript
import { useState, useEffect } from 'react';

// 自定义 Hook
function useApi() {
  const [token, setToken] = useState<string | null>(
    localStorage.getItem('token')
  );
  
  const login = async (email: string, password: string) => {
    const response = await fetch('http://13.113.11.170/auth/login', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ email, password })
    });
    const data = await response.json();
    const newToken = data.data.token;
    setToken(newToken);
    localStorage.setItem('token', newToken);
    return data.data;
  };
  
  const fetchData = async (endpoint: string, params = {}) => {
    const query = new URLSearchParams(params).toString();
    const url = `http://13.113.11.170${endpoint}?${query}`;
    
    const response = await fetch(url, {
      headers: {
        'Authorization': `Bearer ${token}`
      }
    });
    
    if (!response.ok) throw new Error('Request failed');
    return response.json();
  };
  
  return { token, login, fetchData };
}

// 使用示例
function ProfitDashboard() {
  const { token, fetchData } = useApi();
  const [ratios, setRatios] = useState([]);
  const [loading, setLoading] = useState(true);
  
  useEffect(() => {
    if (!token) return;
    
    fetchData('/profit/profit_allocation_ratios', { limit: 10 })
      .then(data => {
        setRatios(data.data);
        setLoading(false);
      })
      .catch(console.error);
  }, [token]);
  
  if (loading) return <div>Loading...</div>;
  
  return (
    <div>
      {ratios.map(ratio => (
        <div key={ratio.id}>
          Portfolio {ratio.portfolioId}: {ratio.toUserRatio * 100}%
        </div>
      ))}
    </div>
  );
}
```

### Python

```python
import requests
from typing import Optional, List, Dict, Any

class FundManagementAPI:
    def __init__(self, base_url: str = "http://13.113.11.170"):
        self.base_url = base_url
        self.token: Optional[str] = None
        self.session = requests.Session()
        self.session.headers.update({'Content-Type': 'application/json'})
    
    def login(self, email: str, password: str) -> Dict[str, Any]:
        """登录并保存 token"""
        response = self.session.post(
            f"{self.base_url}/auth/login",
            json={"email": email, "password": password}
        )
        response.raise_for_status()
        data = response.json()
        self.token = data['data']['token']
        self.session.headers.update({'Authorization': f'Bearer {self.token}'})
        return data['data']
    
    def get_profit_ratios(
        self,
        limit: int = 100,
        offset: int = 0,
        portfolio_ids: Optional[List[int]] = None
    ) -> List[Dict[str, Any]]:
        """获取收益分配比例"""
        params = {'limit': limit, 'offset': offset}
        if portfolio_ids:
            params['portfolio_id'] = portfolio_ids
        
        response = self.session.get(
            f"{self.base_url}/profit/profit_allocation_ratios",
            params=params
        )
        response.raise_for_status()
        return response.json()['data']
    
    def get_withdrawals(
        self,
        limit: int = 100,
        offset: int = 0
    ) -> List[Dict[str, Any]]:
        """获取提现记录"""
        response = self.session.get(
            f"{self.base_url}/profit/profit_withdrawals",
            params={'limit': limit, 'offset': offset}
        )
        response.raise_for_status()
        return response.json()['data']
    
    def get_hourly_profit(
        self,
        user_id: int,
        start_time: Optional[int] = None,
        end_time: Optional[int] = None,
        limit: int = 100
    ) -> List[Dict[str, Any]]:
        """获取用户小时收益"""
        params = {'user_id': user_id, 'limit': limit}
        if start_time:
            params['start_time'] = start_time
        if end_time:
            params['end_time'] = end_time
        
        response = self.session.get(
            f"{self.base_url}/profit/hourly_profit_user",
            params=params
        )
        response.raise_for_status()
        return response.json()['data']

# 使用示例
if __name__ == "__main__":
    api = FundManagementAPI()
    
    # 登录
    user_data = api.login('user@example.com', 'password123')
    print(f"Logged in as: {user_data['username']}")
    
    # 获取收益分配比例
    ratios = api.get_profit_ratios(limit=10)
    for ratio in ratios:
        print(f"Portfolio {ratio['portfolioId']}: "
              f"Team={ratio['toTeamRatio']}, "
              f"Platform={ratio['toPlatformRatio']}, "
              f"User={ratio['toUserRatio']}")
    
    # 获取小时收益
    hourly = api.get_hourly_profit(user_id=1, limit=24)
    total_profit = sum(h['hourProfit'] for h in hourly)
    print(f"Total 24h profit: {total_profit}")
```

---

## 🎯 最佳实践

### 1. Token 管理

```typescript
class TokenManager {
  private static TOKEN_KEY = 'api_token';
  
  static saveToken(token: string) {
    localStorage.setItem(this.TOKEN_KEY, token);
  }
  
  static getToken(): string | null {
    return localStorage.getItem(this.TOKEN_KEY);
  }
  
  static clearToken() {
    localStorage.removeItem(this.TOKEN_KEY);
  }
  
  static isTokenValid(): boolean {
    const token = this.getToken();
    if (!token) return false;
    
    try {
      const payload = JSON.parse(atob(token.split('.')[1]));
      return payload.exp * 1000 > Date.now();
    } catch {
      return false;
    }
  }
}
```

### 2. 错误处理

```typescript
async function apiRequest(url: string, options: RequestInit = {}) {
  try {
    const response = await fetch(url, options);
    
    if (!response.ok) {
      const error = await response.json();
      
      switch (response.status) {
        case 401:
        case 403:
          // Token 过期或无效
          TokenManager.clearToken();
          window.location.href = '/login';
          break;
        case 422:
          // 参数验证错误
          console.error('Validation errors:', error.data.detail);
          break;
        case 429:
          // 请求过于频繁
          await new Promise(resolve => setTimeout(resolve, 1000));
          return apiRequest(url, options); // 重试
        default:
          console.error('API Error:', error.message);
      }
      
      throw new Error(error.message);
    }
    
    return response.json();
  } catch (error) {
    console.error('Network error:', error);
    throw error;
  }
}
```

### 3. 请求重试

```typescript
async function fetchWithRetry(
  url: string,
  options: RequestInit = {},
  retries = 3
): Promise<any> {
  for (let i = 0; i < retries; i++) {
    try {
      return await fetch(url, options);
    } catch (error) {
      if (i === retries - 1) throw error;
      await new Promise(resolve => setTimeout(resolve, 1000 * (i + 1)));
    }
  }
}
```

### 4. 分页处理

```typescript
async function fetchAllPages(endpoint: string, pageSize = 100) {
  const allData = [];
  let offset = 0;
  let hasMore = true;
  
  while (hasMore) {
    const response = await api.request(
      `${endpoint}?limit=${pageSize}&offset=${offset}`
    );
    
    allData.push(...response.data);
    offset += pageSize;
    hasMore = response.data.length === pageSize;
  }
  
  return allData;
}
```

### 5. 缓存策略

```typescript
class ApiCache {
  private cache = new Map<string, { data: any; timestamp: number }>();
  private TTL = 5 * 60 * 1000; // 5分钟
  
  get(key: string): any | null {
    const item = this.cache.get(key);
    if (!item) return null;
    
    if (Date.now() - item.timestamp > this.TTL) {
      this.cache.delete(key);
      return null;
    }
    
    return item.data;
  }
  
  set(key: string, data: any) {
    this.cache.set(key, { data, timestamp: Date.now() });
  }
}
```

### 6. 批量请求

```typescript
async function batchFetch(userIds: number[]) {
  const promises = userIds.map(id =>
    api.fetchData(`/profit/hourly_profit_user`, { user_id: id, limit: 24 })
  );
  
  const results = await Promise.allSettled(promises);
  
  return results.map((result, index) => ({
    userId: userIds[index],
    data: result.status === 'fulfilled' ? result.value : null,
    error: result.status === 'rejected' ? result.reason : null
  }));
}
```

---

## 📱 移动端集成

### React Native

```typescript
import AsyncStorage from '@react-native-async-storage/async-storage';

class MobileApiClient {
  private async getToken() {
    return await AsyncStorage.getItem('token');
  }
  
  async login(email: string, password: string) {
    const response = await fetch('http://13.113.11.170/auth/login', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ email, password })
    });
    
    const data = await response.json();
    await AsyncStorage.setItem('token', data.data.token);
    return data.data;
  }
  
  async fetchData(endpoint: string) {
    const token = await this.getToken();
    
    const response = await fetch(`http://13.113.11.170${endpoint}`, {
      headers: {
        'Authorization': `Bearer ${token}`
      }
    });
    
    return response.json();
  }
}
```

### Flutter

```dart
import 'package:http/http.dart' as http;
import 'package:shared_preferences/shared_preferences.dart';
import 'dart:convert';

class ApiClient {
  static const String baseUrl = 'http://13.113.11.170';
  String? _token;
  
  Future<void> loadToken() async {
    final prefs = await SharedPreferences.getInstance();
    _token = prefs.getString('token');
  }
  
  Future<void> login(String email, String password) async {
    final response = await http.post(
      Uri.parse('$baseUrl/auth/login'),
      headers: {'Content-Type': 'application/json'},
      body: jsonEncode({'email': email, 'password': password}),
    );
    
    final data = jsonDecode(response.body);
    _token = data['data']['token'];
    
    final prefs = await SharedPreferences.getInstance();
    await prefs.setString('token', _token!);
  }
  
  Future<Map<String, dynamic>> fetchData(String endpoint) async {
    final response = await http.get(
      Uri.parse('$baseUrl$endpoint'),
      headers: {
        'Authorization': 'Bearer $_token',
      },
    );
    
    return jsonDecode(response.body);
  }
}
```

---

## 🔄 实时更新

### 轮询方式

```typescript
class PollingService {
  private intervalId: number | null = null;
  
  start(callback: () => Promise<void>, interval = 5000) {
    this.intervalId = window.setInterval(callback, interval);
  }
  
  stop() {
    if (this.intervalId) {
      clearInterval(this.intervalId);
      this.intervalId = null;
    }
  }
}

// 使用
const polling = new PollingService();
polling.start(async () => {
  const data = await api.getProfitRatios({ limit: 10 });
  updateUI(data);
}, 5000); // 每5秒更新一次
```

---

## 🧪 测试

### 单元测试示例

```typescript
import { describe, it, expect, beforeEach } from 'vitest';

describe('ApiClient', () => {
  let api: ApiClient;
  
  beforeEach(() => {
    api = new ApiClient();
  });
  
  it('should login successfully', async () => {
    const token = await api.login('test@example.com', 'password123');
    expect(token).toBeDefined();
    expect(typeof token).toBe('string');
  });
  
  it('should fetch profit ratios', async () => {
    await api.login('test@example.com', 'password123');
    const ratios = await api.getProfitRatios({ limit: 10 });
    expect(Array.isArray(ratios)).toBe(true);
  });
});
```

---

## 📊 性能优化

### 1. 请求合并

```typescript
class RequestBatcher {
  private queue: Array<() => Promise<any>> = [];
  private processing = false;
  
  add(request: () => Promise<any>) {
    this.queue.push(request);
    if (!this.processing) {
      this.process();
    }
  }
  
  private async process() {
    this.processing = true;
    
    while (this.queue.length > 0) {
      const batch = this.queue.splice(0, 10); // 一次处理10个
      await Promise.all(batch.map(req => req()));
    }
    
    this.processing = false;
  }
}
```

### 2. 数据压缩

```typescript
// 请求时使用 gzip 压缩
fetch(url, {
  headers: {
    'Accept-Encoding': 'gzip, deflate'
  }
});
```

---

## 🆘 常见问题

### Q1: CORS 错误

**错误**: `Access to fetch has been blocked by CORS policy`

**解决**: 服务器已配置 CORS，如果仍有问题，请联系后端团队。

### Q2: Token 过期

**错误**: 403 Forbidden

**解决**: 重新登录获取新 Token，建议实现自动刷新机制。

### Q3: 参数验证失败

**错误**: 422 Unprocessable Entity

**解决**: 检查参数类型和范围，参考 API 文档。

---

## 📞 支持

**文档**: http://13.113.11.170/docs  
**联系**: Lucien
**更新日期**: 2025-10-17

---

**祝开发顺利！** 🚀
