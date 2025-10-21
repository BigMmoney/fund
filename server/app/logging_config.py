"""
结构化日志配置
使用loguru替代标准logging，提供更强大的日志功能
"""
import sys
import os
from pathlib import Path
from loguru import logger
import json
from datetime import datetime
from typing import Optional


# ========== 日志配置 ==========

def setup_logging(
    log_level: str = "INFO",
    log_file_path: Optional[str] = None,
    enable_json_logging: bool = False,
    rotation: str = "500 MB",
    retention: str = "30 days",
    compression: str = "zip"
):
    """
    配置loguru日志系统
    
    Args:
        log_level: 日志级别 (DEBUG, INFO, WARNING, ERROR, CRITICAL)
        log_file_path: 日志文件路径
        enable_json_logging: 是否启用JSON格式日志（生产环境推荐）
        rotation: 日志文件轮换策略
        retention: 日志保留时间
        compression: 旧日志压缩格式
    """
    # 移除默认handler
    logger.remove()
    
    # ========== 控制台输出（开发环境）==========
    if not enable_json_logging:
        # 彩色格式化输出（开发环境）
        logger.add(
            sys.stdout,
            format=(
                "<green>{time:YYYY-MM-DD HH:mm:ss.SSS}</green> | "
                "<level>{level: <8}</level> | "
                "<cyan>{name}</cyan>:<cyan>{function}</cyan>:<cyan>{line}</cyan> | "
                "<level>{message}</level>"
            ),
            level=log_level,
            colorize=True,
            backtrace=True,
            diagnose=True,
        )
    else:
        # JSON格式输出（生产环境）
        logger.add(
            sys.stdout,
            format="{message}",
            level=log_level,
            serialize=True,  # 输出为JSON
        )
    
    # ========== 文件输出 ==========
    if log_file_path:
        log_dir = Path(log_file_path).parent
        log_dir.mkdir(parents=True, exist_ok=True)
        
        # 所有日志（包括DEBUG）
        logger.add(
            log_file_path,
            rotation=rotation,
            retention=retention,
            compression=compression,
            format=(
                "{time:YYYY-MM-DD HH:mm:ss.SSS} | {level: <8} | "
                "{extra[trace_id]} | {name}:{function}:{line} | {message}"
            ),
            level="DEBUG",
            serialize=enable_json_logging,
            enqueue=True,  # 异步写入
            backtrace=True,
            diagnose=True,
        )
        
        # 错误日志单独文件
        error_log_path = str(Path(log_file_path).parent / "error.log")
        logger.add(
            error_log_path,
            rotation=rotation,
            retention=retention,
            compression=compression,
            format=(
                "{time:YYYY-MM-DD HH:mm:ss.SSS} | {level: <8} | "
                "{extra[trace_id]} | {name}:{function}:{line} | "
                "{message}\n{exception}"
            ),
            level="ERROR",
            serialize=enable_json_logging,
            enqueue=True,
            backtrace=True,
            diagnose=True,
        )
        
        logger.info(f"Log files configured: {log_file_path}")
    
    # 绑定默认的trace_id
    logger.configure(extra={"trace_id": "N/A"})
    
    logger.info(f"Logging initialized at level {log_level}")


# ========== 请求日志过滤器 ==========

def should_log_request(path: str) -> bool:
    """
    判断是否应该记录请求日志
    
    某些路径（如健康检查）可能产生大量日志，可以过滤
    """
    exclude_paths = [
        "/health",
        "/metrics",
        "/favicon.ico",
    ]
    return path not in exclude_paths


# ========== 日志上下文管理 ==========

class LogContext:
    """日志上下文管理器"""
    
    def __init__(self, **context):
        self.context = context
        self.token = None
    
    def __enter__(self):
        self.token = logger.contextualize(**self.context)
        return self
    
    def __exit__(self, exc_type, exc_val, exc_tb):
        if self.token:
            logger.remove(self.token)


# ========== 慢操作日志装饰器 ==========

import time
from functools import wraps

def log_slow_operation(threshold_seconds: float = 1.0, operation_name: Optional[str] = None):
    """
    记录慢操作的装饰器
    
    Args:
        threshold_seconds: 慢操作阈值（秒）
        operation_name: 操作名称（默认使用函数名）
    
    用法:
        @log_slow_operation(threshold_seconds=0.5, operation_name="数据库查询")
        def my_slow_function():
            pass
    """
    def decorator(func):
        @wraps(func)
        async def async_wrapper(*args, **kwargs):
            op_name = operation_name or func.__name__
            start_time = time.time()
            
            try:
                result = await func(*args, **kwargs)
                duration = time.time() - start_time
                
                if duration > threshold_seconds:
                    logger.warning(
                        f"Slow operation detected: {op_name}",
                        extra={
                            "operation": op_name,
                            "duration_ms": round(duration * 1000, 2),
                            "threshold_ms": threshold_seconds * 1000,
                        }
                    )
                else:
                    logger.debug(
                        f"Operation completed: {op_name}",
                        extra={
                            "operation": op_name,
                            "duration_ms": round(duration * 1000, 2),
                        }
                    )
                
                return result
            
            except Exception as e:
                duration = time.time() - start_time
                logger.error(
                    f"Operation failed: {op_name}",
                    extra={
                        "operation": op_name,
                        "duration_ms": round(duration * 1000, 2),
                        "error": str(e),
                    }
                )
                raise
        
        @wraps(func)
        def sync_wrapper(*args, **kwargs):
            op_name = operation_name or func.__name__
            start_time = time.time()
            
            try:
                result = func(*args, **kwargs)
                duration = time.time() - start_time
                
                if duration > threshold_seconds:
                    logger.warning(
                        f"Slow operation: {op_name} ({duration:.3f}s)",
                        extra={"duration": duration}
                    )
                
                return result
            except Exception as e:
                duration = time.time() - start_time
                logger.error(f"Operation failed: {op_name} ({duration:.3f}s): {e}")
                raise
        
        import asyncio
        if asyncio.iscoroutinefunction(func):
            return async_wrapper
        else:
            return sync_wrapper
    
    return decorator


# ========== 结构化日志辅助函数 ==========

def log_api_call(
    endpoint: str,
    method: str,
    status_code: int,
    duration_ms: float,
    user_id: Optional[str] = None,
    error: Optional[str] = None
):
    """
    记录API调用日志（结构化）
    """
    log_data = {
        "event": "api_call",
        "endpoint": endpoint,
        "method": method,
        "status_code": status_code,
        "duration_ms": duration_ms,
        "user_id": user_id,
    }
    
    if error:
        log_data["error"] = error
        logger.error("API call failed", extra=log_data)
    elif status_code >= 500:
        logger.error("API call server error", extra=log_data)
    elif status_code >= 400:
        logger.warning("API call client error", extra=log_data)
    else:
        logger.info("API call success", extra=log_data)


def log_external_api_call(
    service: str,
    endpoint: str,
    method: str,
    duration_ms: float,
    success: bool,
    error: Optional[str] = None
):
    """
    记录外部API调用日志
    """
    log_data = {
        "event": "external_api_call",
        "service": service,
        "endpoint": endpoint,
        "method": method,
        "duration_ms": duration_ms,
        "success": success,
    }
    
    if error:
        log_data["error"] = error
        logger.error(f"External API call failed: {service}", extra=log_data)
    else:
        logger.info(f"External API call success: {service}", extra=log_data)


def log_database_query(
    query_type: str,
    table: str,
    duration_ms: float,
    rows_affected: Optional[int] = None,
    error: Optional[str] = None
):
    """
    记录数据库查询日志
    """
    log_data = {
        "event": "database_query",
        "query_type": query_type,
        "table": table,
        "duration_ms": duration_ms,
        "rows_affected": rows_affected,
    }
    
    if error:
        log_data["error"] = error
        logger.error("Database query failed", extra=log_data)
    elif duration_ms > 1000:
        logger.warning("Slow database query", extra=log_data)
    else:
        logger.debug("Database query executed", extra=log_data)


def log_business_event(
    event_name: str,
    user_id: Optional[str] = None,
    **kwargs
):
    """
    记录业务事件日志
    
    用法:
        log_business_event(
            "user_withdrawal",
            user_id="123",
            amount=1000.00,
            status="completed"
        )
    """
    log_data = {
        "event": "business_event",
        "event_name": event_name,
        "user_id": user_id,
        "timestamp": datetime.now().isoformat(),
        **kwargs
    }
    
    logger.info(f"Business event: {event_name}", extra=log_data)


# ========== 初始化日志（从环境变量读取配置）==========

def init_logging_from_env():
    """从环境变量初始化日志配置"""
    log_level = os.getenv("LOG_LEVEL", "INFO")
    log_file_path = os.getenv("LOG_FILE_PATH", "/var/log/fund_api/app.log")
    environment = os.getenv("ENVIRONMENT", "development")
    
    # 生产环境启用JSON日志
    enable_json_logging = (environment == "production")
    
    setup_logging(
        log_level=log_level,
        log_file_path=log_file_path if environment != "development" else None,
        enable_json_logging=enable_json_logging
    )


# 自动初始化（导入时）
# init_logging_from_env()
