"""
通用响应模式
"""
from typing import Optional, List, Dict, Any, Generic, TypeVar
from pydantic import BaseModel

T = TypeVar('T')

class BaseResponse(BaseModel, Generic[T]):
    """基础响应模式"""
    isOK: bool
    message: Optional[str] = None
    data: T

class ListResponse(BaseModel):
    """列表响应数据"""
    list: List[Dict[str, Any]]
    total: int

class ObjectResponse(BaseModel):
    """对象响应数据"""
    obj: Dict[str, Any]

class TokenResponse(BaseModel):
    """登录响应数据"""
    obj: Dict[str, Any]
    token: str

# 常用响应类型
class SuccessResponse(BaseResponse[Dict[str, Any]]):
    """成功响应"""
    isOK: bool = True
    message: Optional[str] = None

class ErrorResponse(BaseResponse[Dict[str, Any]]):
    """错误响应"""
    isOK: bool = False
    data: Dict[str, Any] = {}

class ListDataResponse(BaseResponse[ListResponse]):
    """列表数据响应"""
    isOK: bool = True
    message: Optional[str] = None

class ObjectDataResponse(BaseResponse[ObjectResponse]):
    """对象数据响应"""
    isOK: bool = True
    message: Optional[str] = None

class LoginDataResponse(BaseResponse[TokenResponse]):
    """登录数据响应"""
    isOK: bool = True
    message: Optional[str] = None