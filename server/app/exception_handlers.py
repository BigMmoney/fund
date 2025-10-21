"""
全局异常处理器
"""
from fastapi import Request, status
from fastapi.responses import JSONResponse
from fastapi.exceptions import RequestValidationError
from starlette.exceptions import HTTPException as StarletteHTTPException
import traceback
import logging
from typing import Union

from server.app.errors import (
    ApplicationError,
    ErrorCode,
    ErrorResponseBuilder,
    AuthenticationError,
    AuthorizationError,
    BusinessLogicError,
    NotFoundError,
    ValidationError as AppValidationError,
    ExternalServiceError,
    DatabaseError,
    RateLimitError,
)

logger = logging.getLogger(__name__)


async def application_error_handler(request: Request, exc: ApplicationError) -> JSONResponse:
    """
    处理应用异常
    """
    # 记录错误日志
    log_level = logging.ERROR
    
    # 降级某些预期错误的日志级别
    if isinstance(exc, (NotFoundError, AppValidationError)):
        log_level = logging.WARNING
    elif isinstance(exc, AuthenticationError):
        log_level = logging.WARNING
    
    logger.log(
        log_level,
        f"[{exc.trace_id}] {exc.__class__.__name__}: {exc.error_code.message}",
        extra={
            "trace_id": exc.trace_id,
            "error_code": exc.error_code.code,
            "error_message": exc.error_code.message,
            "user_message": exc.user_message,
            "details": exc.details,
            "context": exc.context,
            "path": request.url.path,
            "method": request.method,
            "client_ip": request.client.host if request.client else "unknown",
            "user_agent": request.headers.get("user-agent", ""),
        }
    )
    
    # 构建响应
    response_data = ErrorResponseBuilder.build_error_response(
        exc,
        include_details=False  # 生产环境不返回技术详情
    )
    
    # 确定HTTP状态码
    http_status = exc.error_code.http_status
    
    # 对于业务逻辑错误，统一使用200状态码，由isOK字段标识错误
    if http_status in [400, 404, 409]:
        http_status = 200
    
    return JSONResponse(
        status_code=http_status,
        content=response_data
    )


async def validation_error_handler(request: Request, exc: RequestValidationError) -> JSONResponse:
    """
    处理Pydantic验证错误
    """
    logger.warning(
        f"Validation error on {request.url.path}",
        extra={
            "path": request.url.path,
            "method": request.method,
            "errors": exc.errors(),
            "body": exc.body,
        }
    )
    
    response_data = ErrorResponseBuilder.build_validation_error_response(exc.errors())
    
    return JSONResponse(
        status_code=422,
        content=response_data
    )


async def http_exception_handler(request: Request, exc: StarletteHTTPException) -> JSONResponse:
    """
    处理HTTP异常
    """
    logger.warning(
        f"HTTP {exc.status_code} on {request.url.path}: {exc.detail}",
        extra={
            "path": request.url.path,
            "method": request.method,
            "status_code": exc.status_code,
            "detail": exc.detail,
        }
    )
    
    # 转换为StandardResponse格式
    return JSONResponse(
        status_code=exc.status_code,
        content={
            "isOK": False,
            "message": exc.detail,
            "data": {
                "errorCode": exc.status_code,
            }
        }
    )


async def generic_exception_handler(request: Request, exc: Exception) -> JSONResponse:
    """
    处理所有未捕获的异常
    """
    import uuid
    trace_id = str(uuid.uuid4())
    
    # 记录完整的异常堆栈
    logger.critical(
        f"[{trace_id}] Unhandled Exception: {type(exc).__name__}",
        extra={
            "trace_id": trace_id,
            "exception_type": type(exc).__name__,
            "exception_message": str(exc),
            "traceback": traceback.format_exc(),
            "path": request.url.path,
            "method": request.method,
            "client_ip": request.client.host if request.client else "unknown",
        },
        exc_info=True
    )
    
    # 发送告警（TODO: 集成告警系统）
    # await send_alert_to_ops_team(trace_id, exc, request)
    
    # 返回通用错误消息（不泄露内部信息）
    return JSONResponse(
        status_code=500,
        content={
            "isOK": False,
            "message": "系统暂时出现问题，我们正在紧急处理",
            "data": {
                "errorCode": ErrorCode.INTERNAL_SERVER_ERROR.code,
                "traceId": trace_id,
                "supportContact": "support@company.com"
            }
        }
    )


def register_exception_handlers(app):
    """
    注册所有异常处理器到FastAPI应用
    
    用法:
        from app.main import app
        from app.exception_handlers import register_exception_handlers
        register_exception_handlers(app)
    """
    # 应用异常处理器
    app.add_exception_handler(ApplicationError, application_error_handler)
    
    # 特定应用异常处理器（如果需要特殊处理）
    app.add_exception_handler(AuthenticationError, application_error_handler)
    app.add_exception_handler(AuthorizationError, application_error_handler)
    app.add_exception_handler(BusinessLogicError, application_error_handler)
    app.add_exception_handler(NotFoundError, application_error_handler)
    app.add_exception_handler(AppValidationError, application_error_handler)
    app.add_exception_handler(ExternalServiceError, application_error_handler)
    app.add_exception_handler(DatabaseError, application_error_handler)
    app.add_exception_handler(RateLimitError, application_error_handler)
    
    # FastAPI内置异常处理器
    app.add_exception_handler(RequestValidationError, validation_error_handler)
    app.add_exception_handler(StarletteHTTPException, http_exception_handler)
    
    # 全局异常处理器（捕获所有未处理的异常）
    app.add_exception_handler(Exception, generic_exception_handler)
    
    logger.info("Exception handlers registered successfully")
