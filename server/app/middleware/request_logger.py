"""
请求日志中间件
记录所有 API 请求和响应
"""

import time
import json
import logging
from datetime import datetime
from typing import Callable
from fastapi import Request, Response
from starlette.middleware.base import BaseHTTPMiddleware
from starlette.types import Message

logger = logging.getLogger(__name__)


class RequestLoggingMiddleware(BaseHTTPMiddleware):
    """
    请求日志中间件
    
    记录所有请求的详细信息：
    - 请求路径、方法、参数
    - 响应状态码、耗时
    - 请求和响应体（可选）
    - Trace ID 用于追踪
    """
    
    def __init__(
        self,
        app,
        log_request_body: bool = False,
        log_response_body: bool = False,
        exclude_paths: list = None
    ):
        """
        初始化请求日志中间件
        
        Args:
            app: FastAPI 应用
            log_request_body: 是否记录请求体
            log_response_body: 是否记录响应体
            exclude_paths: 不记录日志的路径列表
        """
        super().__init__(app)
        self.log_request_body = log_request_body
        self.log_response_body = log_response_body
        self.exclude_paths = exclude_paths or ["/health", "/health/live", "/docs", "/openapi.json"]
    
    async def dispatch(self, request: Request, call_next: Callable) -> Response:
        """处理请求"""
        
        # 检查是否在排除列表中
        if any(request.url.path.startswith(path) for path in self.exclude_paths):
            return await call_next(request)
        
        # 获取或生成 trace_id
        trace_id = getattr(request.state, "trace_id", "unknown")
        
        # 记录请求开始
        start_time = time.time()
        
        # 收集请求信息
        request_info = {
            "trace_id": trace_id,
            "timestamp": datetime.utcnow().isoformat() + "Z",
            "method": request.method,
            "path": request.url.path,
            "query_params": dict(request.query_params),
            "client_host": request.client.host if request.client else None,
            "user_agent": request.headers.get("user-agent"),
        }
        
        # 可选：记录请求体
        if self.log_request_body and request.method in ["POST", "PUT", "PATCH"]:
            try:
                body = await request.body()
                if body:
                    request_info["request_body"] = json.loads(body.decode())
            except Exception as e:
                request_info["request_body_error"] = str(e)
        
        # 执行请求
        response = await call_next(request)
        
        # 计算耗时
        duration = time.time() - start_time
        
        # 收集响应信息
        response_info = {
            **request_info,
            "status_code": response.status_code,
            "duration_ms": int(duration * 1000),
        }
        
        # 可选：记录响应体
        if self.log_response_body:
            # 注意：这会读取响应体，需要重新构建响应
            # 在生产环境中可能影响性能
            pass
        
        # 根据状态码选择日志级别
        if response.status_code >= 500:
            logger.error(f"Request failed: {json.dumps(response_info)}")
        elif response.status_code >= 400:
            logger.warning(f"Request error: {json.dumps(response_info)}")
        else:
            logger.info(f"Request completed: {json.dumps(response_info)}")
        
        return response


def setup_request_logging(app, **kwargs):
    """
    设置请求日志中间件
    
    Args:
        app: FastAPI 应用
        **kwargs: 传递给 RequestLoggingMiddleware 的参数
    """
    app.add_middleware(RequestLoggingMiddleware, **kwargs)
    logger.info("Request logging middleware configured")
