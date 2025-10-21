"""
标准响应格式 - 按照API文档规范实现的统一响应格式系统
"""

from typing import Any, Dict, List, Optional, Union
from pydantic import BaseModel
from fastapi import HTTPException
from fastapi.responses import JSONResponse


class StandardResponse:
    """标准响应工具类 - 严格按照API需求文档实现"""
    
    @staticmethod
    def success(data: Any = None) -> Dict[str, Any]:
        """成功响应
        
        response body = {
            isOK: boolean;   //  true：操作成功，或返回正常数据。
            message: string | null ; // isOK = true, 为null
            data: any;       // 数据主体
        }
        """
        return {
            "isOK": True,
            "message": None,
            "data": data if data is not None else {}
        }
    
    @staticmethod
    def error(message: str, data: Any = None) -> Dict[str, Any]:
        """错误响应
        
        response body = {
            isOK: boolean;   //  false: 操作失败或查询失败
            message: string | null ; // isOK = false, 描述具体的错误信息
            data: any;       // 数据主体
        }
        """
        return {
            "isOK": False,
            "message": message,
            "data": data if data is not None else {}
        }
    
    @staticmethod
    def list_success(items: List[Any], total: int) -> Dict[str, Any]:
        """列表成功响应
        
        response body = {
            isOK: boolean;
            message: string | null;
            data: {
                list: object[]  // 当前分页的列表list
                total: number;  // 符合条件的所有项目的数量
            }
        }
        """
        return {
            "isOK": True,
            "message": None,
            "data": {
                "list": items,
                "total": total
            }
        }
    
    @staticmethod
    def object_success(obj: Any) -> Dict[str, Any]:
        """单个对象成功响应
        
        response body = {
            isOK: boolean;
            message: string | null;
            data: {
                 obj: {
                     id: 1,
                     ...
                 }
            }
        }
        """
        return {
            "isOK": True,
            "message": None,
            "data": {
                "obj": obj
            }
        }
    
    @staticmethod
    def login_success(user_obj: Any, token: str) -> Dict[str, Any]:
        """登录成功响应 - 特殊格式
        
        response body = {
            isOK: true;
            message: null;
            data: {
                obj: {
                    id: number;
                    isSuper: boolean;
                    email: string;
                    permissions: string[];
                    suspended: boolean;
                    updatedAt: number;
                    createdAt: number;
                },
                token: string; // 登陆后分配给用户的session token
            }
        }
        """
        return {
            "isOK": True,
            "message": None,
            "data": {
                "obj": user_obj,
                "token": token
            }
        }


class APIException(HTTPException):
    """自定义API异常"""
    
    def __init__(self, message: str, status_code: int = 400):
        super().__init__(status_code=status_code, detail=message)
        self.message = message


class ValidationError(APIException):
    """验证错误"""
    
    def __init__(self, message: str):
        super().__init__(message, 400)


class AuthenticationError(APIException):
    """认证错误"""
    
    def __init__(self, message: str = "Authentication failed"):
        super().__init__(message, 401)


class AuthorizationError(APIException):
    """授权错误"""
    
    def __init__(self, message: str = "Permission denied"):
        super().__init__(message, 403)


class NotFoundError(APIException):
    """资源未找到错误"""
    
    def __init__(self, message: str = "Resource not found"):
        super().__init__(message, 404)


def create_error_handler():
    """创建全局错误处理器"""
    
    async def api_exception_handler(request, exc: APIException):
        """API异常处理"""
        return JSONResponse(
            status_code=exc.status_code,
            content=StandardResponse.error(exc.message)
        )
    
    async def validation_exception_handler(request, exc):
        """验证异常处理"""
        error_messages = []
        for error in exc.errors():
            field = " -> ".join(str(loc) for loc in error["loc"])
            message = error["msg"]
            error_messages.append(f"{field}: {message}")
        
        return JSONResponse(
            status_code=422,
            content=StandardResponse.error(
                f"Validation failed: {'; '.join(error_messages)}"
            )
        )
    
    async def generic_exception_handler(request, exc: Exception):
        """通用异常处理"""
        import logging
        logging.error(f"Unhandled exception: {exc}", exc_info=True)
        
        return JSONResponse(
            status_code=500,
            content=StandardResponse.error("Internal server error")
        )
    
    return {
        APIException: api_exception_handler,
        ValidationError: validation_exception_handler,
        Exception: generic_exception_handler
    }


# 常用响应消息常量
class ResponseMessages:
    # 认证相关
    LOGIN_FAILED = "Login Failed"
    TOKEN_INVALID = "Invalid or expired token"
    TOKEN_MISSING = "Authorization token required"
    PERMISSION_DENIED = "Insufficient permissions"
    
    # 用户相关
    USER_NOT_FOUND = "User not found"
    USER_ALREADY_EXISTS = "User already exists"
    PASSWORD_RESET_SUCCESS = "Password reset successfully"
    PASSWORD_UPDATE_SUCCESS = "Password updated successfully"
    PASSWORD_UPDATE_FAILED = "Password update failed"
    
    # 团队相关
    TEAM_NOT_FOUND = "Team not found"
    TEAM_CREATED_SUCCESS = "Team created successfully"
    TEAM_UPDATED_SUCCESS = "Team updated successfully"
    
    # 投资组合相关
    PORTFOLIO_NOT_FOUND = "Portfolio not found"
    PORTFOLIO_UPDATED_SUCCESS = "Portfolio updated successfully"
    
    # 收益相关
    ALLOCATION_RATIO_CREATED = "Profit allocation ratio created successfully"
    WITHDRAWAL_CREATED = "Withdrawal record created successfully"
    REALLOCATION_CREATED = "Reallocation record created successfully"
    
    # 黑名单相关
    BLACKLIST_ADDED = "Address added to blacklist successfully"
    BLACKLIST_REMOVED = "Address removed from blacklist successfully"
    BLACKLIST_NOT_FOUND = "Blacklist address not found"
    
    # 通用消息
    OPERATION_SUCCESS = "Operation completed successfully"
    VALIDATION_FAILED = "Validation failed"
    RESOURCE_NOT_FOUND = "Resource not found"
    INTERNAL_ERROR = "Internal server error"
