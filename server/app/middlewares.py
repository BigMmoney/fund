"""
请求追踪和监控中间件
"""
import time
import uuid
from typing import Callable
from fastapi import Request, Response
from starlette.middleware.base import BaseHTTPMiddleware
from starlette.types import ASGIApp
from loguru import logger


class RequestTracingMiddleware(BaseHTTPMiddleware):
    """
    请求追踪中间件
    
    功能:
    1. 为每个请求生成唯一的trace_id
    2. 记录请求和响应信息
    3. 计算请求处理时间
    4. 识别慢请求
    5. 将trace_id添加到响应头
    """
    
    def __init__(
        self,
        app: ASGIApp,
        slow_request_threshold: float = 1.0,
        exclude_paths: list = None
    ):
        """
        Args:
            app: ASGI应用
            slow_request_threshold: 慢请求阈值（秒）
            exclude_paths: 排除的路径列表（不记录日志）
        """
        super().__init__(app)
        self.slow_request_threshold = slow_request_threshold
        self.exclude_paths = exclude_paths or ["/health", "/metrics"]
    
    async def dispatch(self, request: Request, call_next: Callable) -> Response:
        # 生成或使用现有的trace_id
        trace_id = request.headers.get("X-Trace-ID") or str(uuid.uuid4())
        
        # 判断是否应该记录日志
        should_log = request.url.path not in self.exclude_paths
        
        # 绑定trace_id到日志上下文
        with logger.contextualize(
            trace_id=trace_id,
            path=request.url.path,
            method=request.method,
            client_ip=request.client.host if request.client else "unknown",
            user_agent=request.headers.get("user-agent", ""),
        ):
            start_time = time.time()
            
            # 记录请求开始
            if should_log:
                logger.info(
                    f"Request started: {request.method} {request.url.path}",
                    extra={
                        "query_params": dict(request.query_params),
                    }
                )
            
            try:
                # 将trace_id添加到request.state，供后续使用
                request.state.trace_id = trace_id
                
                # 处理请求
                response = await call_next(request)
                
                # 计算处理时间
                duration = time.time() - start_time
                duration_ms = round(duration * 1000, 2)
                
                # 将trace_id添加到响应头
                response.headers["X-Trace-ID"] = trace_id
                
                # 记录请求完成
                if should_log:
                    log_level = "info"
                    
                    # 根据状态码和处理时间调整日志级别
                    if response.status_code >= 500:
                        log_level = "error"
                    elif response.status_code >= 400:
                        log_level = "warning"
                    elif duration > self.slow_request_threshold:
                        log_level = "warning"
                    
                    getattr(logger, log_level)(
                        f"Request completed: {request.method} {request.url.path}",
                        extra={
                            "status_code": response.status_code,
                            "duration_ms": duration_ms,
                            "slow_request": duration > self.slow_request_threshold,
                        }
                    )
                
                return response
            
            except Exception as e:
                # 记录异常
                duration = time.time() - start_time
                duration_ms = round(duration * 1000, 2)
                
                logger.error(
                    f"Request failed: {request.method} {request.url.path}",
                    extra={
                        "exception": str(e),
                        "exception_type": type(e).__name__,
                        "duration_ms": duration_ms,
                    },
                    exc_info=True
                )
                
                # 重新抛出异常，让异常处理器处理
                raise


class PerformanceMonitoringMiddleware(BaseHTTPMiddleware):
    """
    性能监控中间件
    
    功能:
    1. 收集请求性能指标
    2. 识别性能瓶颈
    3. 统计API调用次数
    """
    
    def __init__(self, app: ASGIApp):
        super().__init__(app)
        self.request_count = 0
        self.total_duration = 0.0
        self.endpoint_stats = {}  # {endpoint: {count, total_duration, max_duration}}
    
    async def dispatch(self, request: Request, call_next: Callable) -> Response:
        start_time = time.time()
        
        # 处理请求
        response = await call_next(request)
        
        # 计算处理时间
        duration = time.time() - start_time
        
        # 更新统计信息
        self.request_count += 1
        self.total_duration += duration
        
        # 按端点统计
        endpoint = f"{request.method} {request.url.path}"
        if endpoint not in self.endpoint_stats:
            self.endpoint_stats[endpoint] = {
                "count": 0,
                "total_duration": 0.0,
                "max_duration": 0.0,
                "min_duration": float('inf')
            }
        
        stats = self.endpoint_stats[endpoint]
        stats["count"] += 1
        stats["total_duration"] += duration
        stats["max_duration"] = max(stats["max_duration"], duration)
        stats["min_duration"] = min(stats["min_duration"], duration)
        
        # 添加性能头（可选）
        response.headers["X-Response-Time"] = f"{round(duration * 1000, 2)}ms"
        
        return response
    
    def get_stats(self) -> dict:
        """获取性能统计信息"""
        endpoint_summary = {}
        for endpoint, stats in self.endpoint_stats.items():
            endpoint_summary[endpoint] = {
                "count": stats["count"],
                "avg_duration_ms": round((stats["total_duration"] / stats["count"]) * 1000, 2),
                "max_duration_ms": round(stats["max_duration"] * 1000, 2),
                "min_duration_ms": round(stats["min_duration"] * 1000, 2),
            }
        
        return {
            "total_requests": self.request_count,
            "avg_duration_ms": (
                round((self.total_duration / self.request_count) * 1000, 2)
                if self.request_count > 0 else 0
            ),
            "endpoints": endpoint_summary
        }


class SecurityHeadersMiddleware(BaseHTTPMiddleware):
    """
    安全头部中间件
    
    添加常见的安全响应头
    """
    
    async def dispatch(self, request: Request, call_next: Callable) -> Response:
        response = await call_next(request)
        
        # 添加安全头部
        response.headers["X-Content-Type-Options"] = "nosniff"
        response.headers["X-Frame-Options"] = "DENY"
        response.headers["X-XSS-Protection"] = "1; mode=block"
        response.headers["Strict-Transport-Security"] = "max-age=31536000; includeSubDomains"
        
        return response


def register_middlewares(app):
    """
    注册所有中间件
    
    用法:
        from app.main import app
        from app.middlewares import register_middlewares
        register_middlewares(app)
    """
    # 注意：中间件的顺序很重要！
    # 最后添加的中间件最先执行
    
    # 1. 安全头部（最外层）
    app.add_middleware(SecurityHeadersMiddleware)
    
    # 2. 性能监控
    performance_middleware = PerformanceMonitoringMiddleware(app)
    app.add_middleware(PerformanceMonitoringMiddleware)
    
    # 3. 请求追踪（最内层，最先记录）
    app.add_middleware(
        RequestTracingMiddleware,
        slow_request_threshold=1.0,
        exclude_paths=["/health", "/metrics"]
    )
    
    logger.info("Middlewares registered successfully")
    
    # 返回性能监控中间件实例，可用于获取统计信息
    return performance_middleware
