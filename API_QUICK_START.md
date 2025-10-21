# ⚡ API 快速开始指南

**5分钟快速上手 Fund Management API**

---

## 🎯 目标

在 5 分钟内：
1. ✅ 了解 API 基础信息
2. ✅ 完成第一次 API 调用
3. ✅ 理解认证流程
4. ✅ 开始集成开发

---

## 📍 Step 1: 访问 API 文档 (30秒)

打开浏览器访问：

```
http://13.113.11.170/docs
```

你会看到 **Swagger UI** 界面，这是一个交互式 API 文档。

💡 **提示**: 在这里你可以直接测试所有 API！

---

## 🔑 Step 2: 理解认证 (1分钟)

所有 API 都需要 **JWT Token** 认证。

### 认证流程

```
1. POST /auth/login  → 获取 Token
2. 使用 Token       → 访问其他 API
```

### 登录示例

**请求**:
```bash
curl -X POST http://13.113.11.170/auth/login \
  -H "Content-Type: application/json" \
  -d '{"email":"test@example.com","password":"your_password"}'
```

**响应**:
```json
{
  "isOK": true,
  "message": "Login successful",
  "data": {
    "token": "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9...",
    "user": {
      "id": 1,
      "email": "test@example.com",
      "username": "Test User"
    }
  }
}
```

**重要**: 复制 `token` 字段的值，后面会用到。

---

## 🚀 Step 3: 第一次 API 调用 (2分钟)

### 方法 1: 使用 Swagger UI (最简单)

1. 打开 http://13.113.11.170/docs
2. 点击右上角 **"Authorize"** 按钮
3. 输入: `Bearer YOUR_TOKEN_HERE` (注意 Bearer 后面有空格)
4. 点击 **"Authorize"** 确认
5. 找到任意 API，点击 **"Try it out"**
6. 点击 **"Execute"** 执行

✅ 如果看到 200 响应，说明成功了！

### 方法 2: 使用 curl (命令行)

```bash
# 替换 YOUR_TOKEN_HERE 为你的实际 token
curl http://13.113.11.170/profit/profit_allocation_ratios?limit=10 \
  -H "Authorization: Bearer YOUR_TOKEN_HERE"
```

### 方法 3: 使用 JavaScript (前端)

```javascript
const token = "YOUR_TOKEN_HERE";

fetch('http://13.113.11.170/profit/profit_allocation_ratios?limit=10', {
  headers: {
    'Authorization': `Bearer ${token}`
  }
})
.then(res => res.json())
.then(data => console.log(data));
```

---

## 📚 Step 4: 了解核心 API (1.5分钟)

### 主要端点

| API | 用途 | 方法 |
|-----|------|------|
| `/auth/login` | 登录获取 Token | POST |
| `/profit/profit_allocation_ratios` | 获取收益分配比例 | GET |
| `/profit/profit_withdrawals` | 获取提现记录 | GET |
| `/profit/profit_reallocations` | 获取调账记录 | GET |
| `/profit/hourly_profit_user` | 获取用户小时收益 | GET |

### 通用参数

所有列表 API 都支持分页：

```
?limit=100    # 每页数量 (1-1000，默认100)
&offset=0     # 偏移量 (默认0)
```

### 响应格式

**成功**:
```json
{
  "isOK": true,
  "message": "Success",
  "data": [...],
  "total": 100
}
```

**失败**:
```json
{
  "isOK": false,
  "message": "Error message",
  "data": {
    "errorCode": 403
  }
}
```

---

## 💻 Step 5: 完整代码示例 (可选)

### JavaScript/TypeScript

```typescript
// 1. 创建 API 客户端
class ApiClient {
  private baseUrl = 'http://13.113.11.170';
  private token: string = '';

  async login(email: string, password: string) {
    const response = await fetch(`${this.baseUrl}/auth/login`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ email, password })
    });
    
    const data = await response.json();
    this.token = data.data.token;
    return data.data;
  }

  async get(endpoint: string, params: any = {}) {
    const query = new URLSearchParams(params).toString();
    const url = `${this.baseUrl}${endpoint}?${query}`;
    
    const response = await fetch(url, {
      headers: {
        'Authorization': `Bearer ${this.token}`
      }
    });
    
    return response.json();
  }
}

// 2. 使用示例
const api = new ApiClient();

// 登录
await api.login('test@example.com', 'password123');

// 获取数据
const ratios = await api.get('/profit/profit_allocation_ratios', { limit: 10 });
console.log(ratios.data);
```

### Python

```python
import requests

class ApiClient:
    def __init__(self):
        self.base_url = 'http://13.113.11.170'
        self.token = None
    
    def login(self, email, password):
        response = requests.post(
            f'{self.base_url}/auth/login',
            json={'email': email, 'password': password}
        )
        data = response.json()
        self.token = data['data']['token']
        return data['data']
    
    def get(self, endpoint, params=None):
        response = requests.get(
            f'{self.base_url}{endpoint}',
            params=params,
            headers={'Authorization': f'Bearer {self.token}'}
        )
        return response.json()

# 使用示例
api = ApiClient()
api.login('test@example.com', 'password123')

ratios = api.get('/profit/profit_allocation_ratios', {'limit': 10})
print(ratios['data'])
```

---

## 🎯 常见场景

### 场景 1: 获取用户的收益数据

```javascript
// 1. 登录
await api.login('user@example.com', 'password');

// 2. 获取用户最近 24 小时的收益
const hourlyProfit = await api.get('/profit/hourly_profit_user', {
  user_id: 1,
  limit: 24
});

// 3. 计算总收益
const total = hourlyProfit.data.reduce(
  (sum, item) => sum + item.hourProfit, 
  0
);

console.log(`24小时总收益: ${total}`);
```

### 场景 2: 显示收益分配比例

```javascript
// 获取 Portfolio 的分配比例
const ratios = await api.get('/profit/profit_allocation_ratios', {
  portfolio_id: [1]  // 可以传多个
});

ratios.data.forEach(ratio => {
  console.log(`Portfolio ${ratio.portfolioId}:`);
  console.log(`  团队: ${ratio.toTeamRatio * 100}%`);
  console.log(`  平台: ${ratio.toPlatformRatio * 100}%`);
  console.log(`  用户: ${ratio.toUserRatio * 100}%`);
});
```

### 场景 3: 查询提现记录

```javascript
// 获取最近的提现记录
const withdrawals = await api.get('/profit/profit_withdrawals', {
  limit: 20,
  offset: 0
});

withdrawals.data.forEach(w => {
  console.log(`${w.createdAt}: ${w.amount} (${w.status})`);
});
```

---

## ⚠️ 常见错误

### 错误 1: 403 Forbidden

```json
{"isOK": false, "message": "Not authenticated"}
```

**原因**: Token 缺失或无效  
**解决**: 检查 Authorization Header 是否正确

### 错误 2: 422 Validation Error

```json
{
  "detail": [
    {"loc": ["query", "limit"], "msg": "ensure this value is less than or equal to 1000"}
  ]
}
```

**原因**: 参数验证失败  
**解决**: 检查参数类型和范围

### 错误 3: CORS Error

```
Access to fetch has been blocked by CORS policy
```

**原因**: 跨域请求被阻止  
**解决**: 服务器已配置 CORS，如仍有问题请联系后端

---

## 📖 下一步

完成快速开始后，你可以：

1. 📚 阅读 [完整 API 文档](./API_DOCUMENTATION_COMPLETE.md)
2. 🧪 在 [Swagger UI](http://13.113.11.170/docs) 中测试所有端点
3. 💻 查看更多 [代码示例](./API_DOCUMENTATION_COMPLETE.md#代码示例)
4. 🏗️ 了解 [项目技术架构](./PROJECT_TECHNICAL_DOCUMENTATION.md)

---

## 🆘 获取帮助

- **在线文档**: http://13.113.11.170/docs
- **技术文档**: [README.md](./README.md)
- **联系支持**: Lucien

---

## ✅ 检查清单

完成快速开始后，确认：

- [ ] 能访问 Swagger UI
- [ ] 成功登录并获取 Token
- [ ] 能调用至少一个 API
- [ ] 理解响应格式
- [ ] 能处理错误情况

---

**恭喜！你已经掌握了基础用法** 🎉

现在可以开始集成开发了。如有问题，查看 [完整文档](./API_DOCUMENTATION_COMPLETE.md) 或联系技术团队。
