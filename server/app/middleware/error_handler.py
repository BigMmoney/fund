"""
统一的错误处理中间件和异常处理器
"""

import traceback
import uuid
from datetime import datetime
from typing import Optional
from fastapi import Request, HTTPException, status
from fastapi.responses import JSONResponse
from starlette.middleware.base import BaseHTTPMiddleware
import logging

logger = logging.getLogger(__name__)


class APIException(Exception):
    """自定义 API 异常基类"""
    
    def __init__(
        self,
        message: str,
        error_code: str,
        status_code: int = 500,
        details: Optional[dict] = None
    ):
        self.message = message
        self.error_code = error_code
        self.status_code = status_code
        self.details = details or {}
        super().__init__(self.message)


class OneTokenAPIError(APIException):
    """OneToken API 错误"""
    
    def __init__(self, message: str, details: Optional[dict] = None):
        super().__init__(
            message=message,
            error_code="ONETOKEN_ERROR",
            status_code=502,
            details=details
        )


class CeffuAPIError(APIException):
    """Ceffu API 错误"""
    
    def __init__(self, message: str, details: Optional[dict] = None):
        super().__init__(
            message=message,
            error_code="CEFFU_ERROR",
            status_code=502,
            details=details
        )


class ValidationError(APIException):
    """参数验证错误"""
    
    def __init__(self, message: str, details: Optional[dict] = None):
        super().__init__(
            message=message,
            error_code="VALIDATION_ERROR",
            status_code=400,
            details=details
        )


class ErrorResponse:
    """统一的错误响应格式"""
    
    @staticmethod
    def format_error(
        message: str,
        error_code: str,
        status_code: int,
        trace_id: str,
        details: Optional[dict] = None,
        path: Optional[str] = None
    ) -> dict:
        """
        格式化错误响应
        
        Returns:
            {
                "success": false,
                "error": {
                    "code": "ERROR_CODE",
                    "message": "Error message",
                    "details": {...},
                    "trace_id": "uuid",
                    "timestamp": "2025-10-12T10:00:00Z",
                    "path": "/api/..."
                }
            }
        """
        return {
            "success": False,
            "error": {
                "code": error_code,
                "message": message,
                "details": details or {},
                "trace_id": trace_id,
                "timestamp": datetime.utcnow().isoformat() + "Z",
                "path": path
            }
        }


class ErrorHandlerMiddleware(BaseHTTPMiddleware):
    """错误处理中间件 - 为每个请求添加 trace_id"""
    
    async def dispatch(self, request: Request, call_next):
        # 生成 trace_id
        trace_id = str(uuid.uuid4())
        request.state.trace_id = trace_id
        
        try:
            response = await call_next(request)
            # 添加 trace_id 到响应头
            response.headers["X-Trace-ID"] = trace_id
            return response
            
        except Exception as e:
            # 记录异常
            logger.error(
                f"Unhandled exception in request {trace_id}: {str(e)}\n"
                f"Traceback: {traceback.format_exc()}"
            )
            
            # 返回统一错误响应
            return JSONResponse(
                status_code=500,
                content=ErrorResponse.format_error(
                    message="Internal server error",
                    error_code="INTERNAL_ERROR",
                    status_code=500,
                    trace_id=trace_id,
                    details={"exception": str(e)},
                    path=request.url.path
                ),
                headers={"X-Trace-ID": trace_id}
            )


# 异常处理器
async def api_exception_handler(request: Request, exc: APIException) -> JSONResponse:
    """处理自定义 API 异常"""
    trace_id = getattr(request.state, "trace_id", str(uuid.uuid4()))
    
    logger.warning(
        f"API Exception [{trace_id}]: {exc.error_code} - {exc.message}"
    )
    
    return JSONResponse(
        status_code=exc.status_code,
        content=ErrorResponse.format_error(
            message=exc.message,
            error_code=exc.error_code,
            status_code=exc.status_code,
            trace_id=trace_id,
            details=exc.details,
            path=request.url.path
        ),
        headers={"X-Trace-ID": trace_id}
    )


async def http_exception_handler(request: Request, exc: HTTPException) -> JSONResponse:
    """处理 HTTP 异常"""
    trace_id = getattr(request.state, "trace_id", str(uuid.uuid4()))
    
    # 映射 HTTP 状态码到错误码
    error_code_map = {
        400: "BAD_REQUEST",
        401: "UNAUTHORIZED",
        403: "FORBIDDEN",
        404: "NOT_FOUND",
        405: "METHOD_NOT_ALLOWED",
        422: "UNPROCESSABLE_ENTITY",
        429: "TOO_MANY_REQUESTS",
        500: "INTERNAL_ERROR",
        502: "BAD_GATEWAY",
        503: "SERVICE_UNAVAILABLE",
        504: "GATEWAY_TIMEOUT"
    }
    
    error_code = error_code_map.get(exc.status_code, "HTTP_ERROR")
    
    logger.warning(
        f"HTTP Exception [{trace_id}]: {exc.status_code} - {exc.detail}"
    )
    
    return JSONResponse(
        status_code=exc.status_code,
        content=ErrorResponse.format_error(
            message=str(exc.detail),
            error_code=error_code,
            status_code=exc.status_code,
            trace_id=trace_id,
            path=request.url.path
        ),
        headers={"X-Trace-ID": trace_id}
    )


async def validation_exception_handler(request: Request, exc: Exception) -> JSONResponse:
    """处理 Pydantic 验证异常"""
    trace_id = getattr(request.state, "trace_id", str(uuid.uuid4()))
    
    # 提取验证错误详情
    if hasattr(exc, "errors"):
        validation_errors = []
        for error in exc.errors():
            validation_errors.append({
                "field": ".".join(str(x) for x in error["loc"]),
                "message": error["msg"],
                "type": error["type"]
            })
        details = {"validation_errors": validation_errors}
    else:
        details = {"message": str(exc)}
    
    logger.warning(
        f"Validation Exception [{trace_id}]: {details}"
    )
    
    return JSONResponse(
        status_code=422,
        content=ErrorResponse.format_error(
            message="Request validation failed",
            error_code="VALIDATION_ERROR",
            status_code=422,
            trace_id=trace_id,
            details=details,
            path=request.url.path
        ),
        headers={"X-Trace-ID": trace_id}
    )


async def generic_exception_handler(request: Request, exc: Exception) -> JSONResponse:
    """处理其他未捕获的异常"""
    trace_id = getattr(request.state, "trace_id", str(uuid.uuid4()))
    
    logger.error(
        f"Unhandled Exception [{trace_id}]: {str(exc)}\n"
        f"Traceback: {traceback.format_exc()}"
    )
    
    return JSONResponse(
        status_code=500,
        content=ErrorResponse.format_error(
            message="An unexpected error occurred",
            error_code="INTERNAL_ERROR",
            status_code=500,
            trace_id=trace_id,
            details={"exception": str(exc)} if logger.level == logging.DEBUG else {},
            path=request.url.path
        ),
        headers={"X-Trace-ID": trace_id}
    )


def register_exception_handlers(app):
    """注册所有异常处理器到 FastAPI 应用"""
    from fastapi.exceptions import RequestValidationError
    from starlette.exceptions import HTTPException as StarletteHTTPException
    
    # 添加错误处理中间件
    app.add_middleware(ErrorHandlerMiddleware)
    
    # 注册异常处理器
    app.add_exception_handler(APIException, api_exception_handler)
    app.add_exception_handler(HTTPException, http_exception_handler)
    app.add_exception_handler(StarletteHTTPException, http_exception_handler)  # 捕获 Starlette 的 404
    app.add_exception_handler(RequestValidationError, validation_exception_handler)
    app.add_exception_handler(Exception, generic_exception_handler)
    
    logger.info("Exception handlers registered successfully")
