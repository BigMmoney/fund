"""
用户相关模式
"""
from typing import List, Optional
from pydantic import BaseModel, EmailStr

class UserBase(BaseModel):
    """用户基础模式"""
    email: EmailStr
    permissions: List[str] = []

class UserCreate(UserBase):
    """创建用户模式"""
    pass

class UserUpdate(BaseModel):
    """更新用户模式"""
    permissions: List[str]

class UserPasswordUpdate(BaseModel):
    """用户密码更新模式"""
    old_password: str
    new_password: str

class UserPasswordReset(BaseModel):
    """用户密码重置模式（空body）"""
    pass

class UserSuspend(BaseModel):
    """用户禁用模式（空body）"""
    pass

class UserResponse(BaseModel):
    """用户响应模式"""
    id: int
    is_super: bool
    email: str
    permissions: List[str]
    suspended: bool
    updated_at: int
    created_at: int

    class Config:
        from_attributes = True

class LoginRequest(BaseModel):
    """登录请求模式"""
    email: EmailStr
    password: str

class LoginResponse(UserResponse):
    """登录响应模式"""
    pass