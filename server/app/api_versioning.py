"""
API版本控制 - v1路由结构
实现向后兼容的API版本化
"""
from fastapi import APIRouter, Request
from fastapi.responses import JSONResponse

# 创建v1版本路由器
api_v1_router = APIRouter(prefix="/api/v1")

# 健康检查路由（v1）
@api_v1_router.get("/health")
async def health_check_v1():
    """
    健康检查端点 - v1
    
    返回：
    - status: 服务状态
    - version: API版本
    - features: 可用功能列表
    """
    return {
        "status": "ok",
        "api_version": "v1",
        "app_version": "1.0.0",
        "features": {
            "trace_id": True,
            "rate_limiting": True,
            "caching": False,  # P1功能，待实现
            "async_tasks": False,  # P1功能，待实现
            "monitoring": False,  # P1功能，待实现
        },
        "deprecation": None  # 无弃用警告
    }

# 根路由返回API信息
@api_v1_router.get("/")
async def api_v1_root():
    """
    API v1 根路径
    
    返回API版本信息和可用端点列表
    """
    return {
        "message": "Fund Management API - Version 1",
        "version": "v1",
        "documentation": "/docs",
        "endpoints": {
            "health": "/api/v1/health",
            "portfolios": "/api/v1/portfolios",
            "teams": "/api/v1/teams",
            "users": "/api/v1/users",
            "profits": "/api/v1/profits",
        },
        "features": [
            "Request tracing (trace_id)",
            "Rate limiting",
            "Error handling with standardized codes",
            "Security headers",
            "Performance monitoring"
        ]
    }

# 版本弃用中间件
class APIVersionMiddleware:
    """
    API版本控制中间件
    
    功能：
    1. 检测请求的API版本
    2. 添加版本相关的响应头
    3. 处理版本弃用警告
    """
    
    def __init__(self, app):
        self.app = app
        self.deprecated_versions = []  # 已弃用的版本
        self.sunset_versions = {}  # 计划停用的版本及日期
    
    async def __call__(self, scope, receive, send):
        if scope["type"] == "http":
            # 提取请求路径
            path = scope["path"]
            
            # 检测API版本
            api_version = self._extract_version(path)
            
            # 修改send函数以添加版本头
            async def send_with_version_headers(message):
                if message["type"] == "http.response.start":
                    headers = list(message.get("headers", []))
                    
                    # 添加API版本头
                    if api_version:
                        headers.append((b"X-API-Version", api_version.encode()))
                    
                    # 添加弃用警告
                    if api_version in self.deprecated_versions:
                        headers.append((
                            b"X-API-Deprecation",
                            b"This API version is deprecated. Please migrate to v1."
                        ))
                    
                    # 添加停用日期
                    if api_version in self.sunset_versions:
                        sunset_date = self.sunset_versions[api_version]
                        headers.append((
                            b"Sunset",
                            sunset_date.encode()
                        ))
                    
                    message["headers"] = headers
                
                await send(message)
            
            await self.app(scope, receive, send_with_version_headers)
        else:
            await self.app(scope, receive, send)
    
    def _extract_version(self, path: str) -> str:
        """从路径中提取API版本"""
        if path.startswith("/api/v1/"):
            return "v1"
        elif path.startswith("/api/v2/"):
            return "v2"
        return "legacy"
    
    def deprecate_version(self, version: str):
        """标记某个版本为已弃用"""
        if version not in self.deprecated_versions:
            self.deprecated_versions.append(version)
    
    def set_sunset_date(self, version: str, sunset_date: str):
        """设置版本停用日期（RFC 5988格式）"""
        self.sunset_versions[version] = sunset_date


# 向后兼容的路由别名
def create_legacy_routes(api_v1_router: APIRouter):
    """
    为旧的API端点创建向后兼容的路由
    这些路由会重定向到v1版本，并添加弃用警告
    """
    legacy_router = APIRouter()
    
    @legacy_router.get("/health")
    async def legacy_health(request: Request):
        """
        旧版健康检查（向后兼容）
        重定向到 /api/v1/health
        """
        return JSONResponse(
            content={
                "status": "ok",
                "warning": "This endpoint is deprecated. Please use /api/v1/health",
                "redirect": "/api/v1/health"
            },
            headers={
                "X-API-Deprecation": "true",
                "X-API-Migration-Guide": "https://docs.example.com/migration/v1"
            }
        )
    
    return legacy_router


# API版本策略类
class APIVersionStrategy:
    """
    API版本管理策略
    
    支持的版本策略：
    1. URL路径版本（推荐）: /api/v1/resource
    2. Header版本: Accept: application/vnd.api+json; version=1
    3. 查询参数版本: /resource?version=1
    """
    
    @staticmethod
    def from_path(path: str) -> str:
        """从路径提取版本"""
        if "/api/v" in path:
            version = path.split("/api/v")[1].split("/")[0]
            return f"v{version}"
        return "legacy"
    
    @staticmethod
    def from_header(headers: dict) -> str:
        """从Accept头提取版本"""
        accept = headers.get("accept", "")
        if "version=" in accept:
            version = accept.split("version=")[1].split(";")[0].strip()
            return f"v{version}"
        return "v1"
    
    @staticmethod
    def from_query(query_params: dict) -> str:
        """从查询参数提取版本"""
        return query_params.get("version", "v1")


__all__ = [
    "api_v1_router",
    "APIVersionMiddleware",
    "create_legacy_routes",
    "APIVersionStrategy"
]
