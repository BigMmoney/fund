"""
P1-4: API文档增强
提供详细的API文档、示例、错误码说明和认证流程
"""
from fastapi import FastAPI
from typing import Dict, List, Any

# API文档配置
API_DOCUMENTATION = {
    "title": "Fund Management API",
    "version": "1.0.0",
    "description": """
# 基金管理系统 API 文档

## 概述
这是一个完整的基金管理系统API，提供投资组合管理、团队协作、收益分配等功能。

## 特性
- ✅ **P0功能**：请求追踪、限流保护、错误处理、日志系统
- ✅ **P1功能**：API版本控制、增强文档
- ⏳ **开发中**：缓存层、异步任务、监控告警

## 认证方式
使用 JWT Bearer Token 进行认证：
```
Authorization: Bearer <your_jwt_token>
```

## 限流规则
- 默认：100次/分钟，1000次/小时
- 登录接口：5次/分钟
- 提现接口：10次/天

## 响应格式
所有API响应遵循统一格式：

### 成功响应
```json
{
  "isOK": true,
  "message": null,
  "data": {
    // 实际数据
  }
}
```

### 错误响应
```json
{
  "isOK": false,
  "message": "错误描述",
  "data": {
    "errorCode": 1001,
    "details": "详细错误信息"
  }
}
```

## 错误码说明
详见下方"错误码参考"部分。
    """,
    "contact": {
        "name": "API Support",
        "email": "support@fundmanagement.com",
    },
    "license_info": {
        "name": "MIT License",
    },
}

# 标签元数据（用于文档分组）
TAGS_METADATA = [
    {
        "name": "健康检查",
        "description": "系统健康状态检查端点",
    },
    {
        "name": "认证",
        "description": """
        用户认证相关接口
        
        ## 认证流程
        1. **登录** - POST /api/v1/auth/login
           - 提供邮箱和密码
           - 返回JWT token
        2. **使用Token** - 在后续请求中添加Header
           ```
           Authorization: Bearer <token>
           ```
        3. **刷新Token** - POST /api/v1/auth/refresh
        4. **登出** - POST /api/v1/auth/logout
        
        ## 示例
        ```python
        # 登录
        response = requests.post(
            "http://localhost:8002/api/v1/auth/login",
            json={"email": "user@example.com", "password": "password123"}
        )
        token = response.json()["data"]["token"]
        
        # 使用token访问受保护的接口
        headers = {"Authorization": f"Bearer {token}"}
        response = requests.get(
            "http://localhost:8002/api/v1/portfolios",
            headers=headers
        )
        ```
        """,
    },
    {
        "name": "投资组合",
        "description": """
        投资组合管理接口
        
        ## 功能
        - 创建投资组合
        - 查询投资组合列表
        - 获取投资组合详情
        - 更新投资组合信息
        - 删除投资组合
        
        ## 示例
        ### 创建投资组合
        ```python
        response = requests.post(
            "http://localhost:8002/api/v1/portfolios",
            headers={"Authorization": f"Bearer {token}"},
            json={
                "name": "科技股投资组合",
                "description": "专注科技股的投资组合",
                "allocation_percentage": 30.0
            }
        )
        ```
        
        ### 查询列表（带分页）
        ```python
        response = requests.get(
            "http://localhost:8002/api/v1/portfolios?page=1&page_size=20",
            headers={"Authorization": f"Bearer {token}"}
        )
        ```
        """,
    },
    {
        "name": "团队管理",
        "description": """
        团队协作管理接口
        
        ## 权限说明
        - **超级管理员**：所有权限
        - **团队管理员**：管理本团队成员和投资组合
        - **普通成员**：查看权限
        """,
    },
    {
        "name": "用户管理",
        "description": "用户账户管理接口",
    },
    {
        "name": "收益管理",
        "description": """
        收益计算和分配接口
        
        ## 收益流程
        1. 系统自动计算收益（每小时）
        2. 查询收益数据
        3. 设置分配比例
        4. 执行收益分配
        5. 记录分配历史
        """,
    },
]

# 错误码完整参考
ERROR_CODES_REFERENCE = {
    "1xxx": {
        "category": "认证错误",
        "codes": {
            1001: {"message": "未提供认证token", "http_status": 401},
            1002: {"message": "token已过期", "http_status": 401},
            1003: {"message": "token无效", "http_status": 401},
            1004: {"message": "用户名或密码错误", "http_status": 401},
            1005: {"message": "账户已被禁用", "http_status": 403},
        }
    },
    "2xxx": {
        "category": "授权错误",
        "codes": {
            2001: {"message": "权限不足", "http_status": 403},
            2002: {"message": "需要超级管理员权限", "http_status": 403},
            2003: {"message": "需要团队管理权限", "http_status": 403},
        }
    },
    "3xxx": {
        "category": "资源错误",
        "codes": {
            3001: {"message": "资源不存在", "http_status": 404},
            3002: {"message": "投资组合不存在", "http_status": 404},
            3003: {"message": "团队不存在", "http_status": 404},
            3004: {"message": "用户不存在", "http_status": 404},
        }
    },
    "4xxx": {
        "category": "验证错误",
        "codes": {
            4001: {"message": "请求参数无效", "http_status": 400},
            4002: {"message": "邮箱格式无效", "http_status": 400},
            4003: {"message": "密码长度不足", "http_status": 400},
            4004: {"message": "分配比例超出范围", "http_status": 400},
        }
    },
    "5xxx": {
        "category": "业务逻辑错误",
        "codes": {
            5001: {"message": "资源已存在", "http_status": 409},
            5002: {"message": "邮箱已被使用", "http_status": 409},
            5003: {"message": "余额不足", "http_status": 400},
            5004: {"message": "操作冲突", "http_status": 409},
        }
    },
    "9xxx": {
        "category": "系统错误",
        "codes": {
            9001: {"message": "数据库错误", "http_status": 500},
            9002: {"message": "外部API调用失败", "http_status": 502},
            9003: {"message": "系统繁忙，请稍后重试", "http_status": 503},
        }
    }
}

# API示例代码
API_EXAMPLES = {
    "python": """
# Python SDK示例

import requests

class FundManagementClient:
    def __init__(self, base_url="http://localhost:8002", token=None):
        self.base_url = base_url
        self.token = token
        self.session = requests.Session()
        if token:
            self.session.headers.update({"Authorization": f"Bearer {token}"})
    
    def login(self, email: str, password: str):
        '''登录并获取token'''
        response = self.session.post(
            f"{self.base_url}/api/v1/auth/login",
            json={"email": email, "password": password}
        )
        data = response.json()
        if data["isOK"]:
            self.token = data["data"]["token"]
            self.session.headers.update({"Authorization": f"Bearer {self.token}"})
        return data
    
    def get_portfolios(self, page=1, page_size=20):
        '''获取投资组合列表'''
        response = self.session.get(
            f"{self.base_url}/api/v1/portfolios",
            params={"page": page, "page_size": page_size}
        )
        return response.json()
    
    def create_portfolio(self, name: str, description: str = None, allocation: float = 0):
        '''创建投资组合'''
        response = self.session.post(
            f"{self.base_url}/api/v1/portfolios",
            json={
                "name": name,
                "description": description,
                "allocation_percentage": allocation
            }
        )
        return response.json()

# 使用示例
client = FundManagementClient()
client.login("admin@example.com", "password123")
portfolios = client.get_portfolios()
print(f"共有 {portfolios['data']['total']} 个投资组合")
    """,
    
    "javascript": """
// JavaScript/Node.js SDK示例

class FundManagementClient {
    constructor(baseUrl = 'http://localhost:8002', token = null) {
        this.baseUrl = baseUrl;
        this.token = token;
    }
    
    async login(email, password) {
        const response = await fetch(`${this.baseUrl}/api/v1/auth/login`, {
            method: 'POST',
            headers: {'Content-Type': 'application/json'},
            body: JSON.stringify({email, password})
        });
        const data = await response.json();
        if (data.isOK) {
            this.token = data.data.token;
        }
        return data;
    }
    
    async getPortfolios(page = 1, pageSize = 20) {
        const response = await fetch(
            `${this.baseUrl}/api/v1/portfolios?page=${page}&page_size=${pageSize}`,
            {headers: {'Authorization': `Bearer ${this.token}`}}
        );
        return await response.json();
    }
    
    async createPortfolio(name, description = null, allocation = 0) {
        const response = await fetch(`${this.baseUrl}/api/v1/portfolios`, {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json',
                'Authorization': `Bearer ${this.token}`
            },
            body: JSON.stringify({
                name,
                description,
                allocation_percentage: allocation
            })
        });
        return await response.json();
    }
}

// 使用示例
const client = new FundManagementClient();
await client.login('admin@example.com', 'password123');
const portfolios = await client.getPortfolios();
console.log(`共有 ${portfolios.data.total} 个投资组合`);
    """,
    
    "curl": """
# cURL命令示例

# 1. 登录获取token
curl -X POST http://localhost:8002/api/v1/auth/login \\
  -H "Content-Type: application/json" \\
  -d '{"email": "admin@example.com", "password": "password123"}'

# 响应：{"isOK": true, "data": {"token": "eyJ..."}}

# 2. 使用token获取投资组合列表
TOKEN="eyJ..."
curl -X GET http://localhost:8002/api/v1/portfolios?page=1&page_size=20 \\
  -H "Authorization: Bearer $TOKEN"

# 3. 创建新投资组合
curl -X POST http://localhost:8002/api/v1/portfolios \\
  -H "Authorization: Bearer $TOKEN" \\
  -H "Content-Type: application/json" \\
  -d '{
    "name": "科技股组合",
    "description": "专注科技股投资",
    "allocation_percentage": 30.0
  }'

# 4. 获取投资组合详情
curl -X GET http://localhost:8002/api/v1/portfolios/123 \\
  -H "Authorization: Bearer $TOKEN"

# 5. 更新投资组合
curl -X PATCH http://localhost:8002/api/v1/portfolios/123 \\
  -H "Authorization: Bearer $TOKEN" \\
  -H "Content-Type: application/json" \\
  -d '{"allocation_percentage": 35.0}'

# 6. 删除投资组合
curl -X DELETE http://localhost:8002/api/v1/portfolios/123 \\
  -H "Authorization: Bearer $TOKEN"
    """
}

def configure_api_docs(app: FastAPI) -> FastAPI:
    """
    配置增强的API文档
    
    Args:
        app: FastAPI应用实例
    
    Returns:
        配置后的FastAPI应用
    """
    # 更新应用配置
    app.title = API_DOCUMENTATION["title"]
    app.version = API_DOCUMENTATION["version"]
    app.description = API_DOCUMENTATION["description"]
    app.contact = API_DOCUMENTATION.get("contact")
    app.license_info = API_DOCUMENTATION.get("license_info")
    
    # 添加标签元数据
    app.openapi_tags = TAGS_METADATA
    
    # 添加自定义路由展示错误码和示例
    @app.get("/api/v1/docs/errors", tags=["文档"], include_in_schema=True)
    async def get_error_codes():
        """
        获取完整的错误码参考
        
        返回所有API可能返回的错误码及其说明
        """
        return {
            "isOK": True,
            "message": None,
            "data": {
                "error_codes": ERROR_CODES_REFERENCE,
                "usage": "根据errorCode字段判断具体错误类型"
            }
        }
    
    @app.get("/api/v1/docs/examples", tags=["文档"], include_in_schema=True)
    async def get_api_examples():
        """
        获取API使用示例代码
        
        提供Python、JavaScript和cURL的示例代码
        """
        return {
            "isOK": True,
            "message": None,
            "data": {
                "examples": API_EXAMPLES,
                "note": "示例代码展示了如何使用API进行常见操作"
            }
        }
    
    @app.get("/api/v1/docs/authentication", tags=["文档"], include_in_schema=True)
    async def get_auth_guide():
        """
        获取认证流程指南
        
        详细说明如何进行身份认证
        """
        return {
            "isOK": True,
            "message": None,
            "data": {
                "authentication_flow": {
                    "step1": {
                        "title": "登录获取Token",
                        "endpoint": "POST /api/v1/auth/login",
                        "request": {
                            "email": "user@example.com",
                            "password": "password123"
                        },
                        "response": {
                            "isOK": True,
                            "data": {
                                "token": "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9...",
                                "expires_in": 86400
                            }
                        }
                    },
                    "step2": {
                        "title": "在后续请求中使用Token",
                        "header": "Authorization: Bearer <token>",
                        "example": "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9..."
                    },
                    "step3": {
                        "title": "Token过期时刷新",
                        "endpoint": "POST /api/v1/auth/refresh",
                        "note": "在token过期前调用此接口获取新token"
                    }
                },
                "security_notes": [
                    "Token默认24小时过期",
                    "请妥善保管token，不要泄露",
                    "建议使用HTTPS传输",
                    "定期轮换密码"
                ]
            }
        }
    
    return app


__all__ = [
    "API_DOCUMENTATION",
    "TAGS_METADATA",
    "ERROR_CODES_REFERENCE",
    "API_EXAMPLES",
    "configure_api_docs"
]
