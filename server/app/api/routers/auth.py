"""
认证路由 - 实现API [02-05]
完全按照需求文档规范实现认证系统
"""
from typing import Dict, Any, Optional
from fastapi import APIRouter, Depends, HTTPException, Request
from sqlalchemy.orm import Session
from pydantic import BaseModel, validator

from server.app.database import get_db
from server.app.models import User, UserSession
from server.app.auth import AuthService, get_current_user
from server.app.responses import (
    StandardResponse, ResponseMessages, ValidationError, 
    AuthenticationError, format_user_data
)

router = APIRouter(prefix="/auth", tags=["Authentication"])

# 认证服务实例
auth_service = AuthService()

# 请求模型
class LoginRequest(BaseModel):
    """API [02] - 登录请求模型"""
    email: str
    password: str
    
    @validator('email')
    def validate_email(cls, v):
        if not v or '@' not in v:
            raise ValueError('Valid email required')
        return v.lower()
    
    @validator('password')
    def validate_password(cls, v):
        if not v or len(v) < 6:
            raise ValueError('Password must be at least 6 characters')
        return v


class PasswordChangeRequest(BaseModel):
    """API [05] - 修改密码请求模型"""
    old_password: str
    new_password: str
    
    @validator('new_password')
    def validate_new_password(cls, v):
        if not v or len(v) < 6:
            raise ValueError('New password must be at least 6 characters')
        return v


# API [02] - 用户登录
@router.post("/login")
async def login(request: LoginRequest, db: Session = Depends(get_db)) -> Dict[str, Any]:
    """
    API [02] - 验证用户和密码，生成session token
    """
    try:
        # 验证用户凭据
        result = auth_service.authenticate_user(
            db, request.email, request.password
        )
        
        if not result["success"]:
            return create_error_response(ResponseMessages.LOGIN_FAILED)
        
        user = result["user"]
        token = result["token"]
        
        # 格式化用户数据
        user_data = format_user_data(user)
        
        return create_success_response({
            "obj": user_data,
            "token": token
        })
        
    except Exception as e:
        return create_error_response(ResponseMessages.LOGIN_FAILED)


# API [03] - 检查当前登录状态
@router.get("/current")
async def get_current(
    current_user: User = Depends(get_current_user)
) -> Dict[str, Any]:
    """
    API [03] - 检测用户的token，查看当前的登录状态
    """
    try:
        user_data = format_user_data(current_user)
        
        return create_success_response({
            "obj": user_data
        })
        
    except Exception as e:
        return create_error_response(ResponseMessages.UNAUTHORIZED)


# API [04] - 用户退出登录
@router.post("/logout")
async def logout(
    current_user: User = Depends(get_current_user),
    db: Session = Depends(get_db)
) -> Dict[str, Any]:
    """
    API [04] - 用户退出登录，删除服务器上的token记录
    """
    try:
        # 删除当前用户的所有会话
        db.query(UserSession).filter(
            UserSession.user_id == current_user.id
        ).delete()
        
        db.commit()
        
        return create_success_response({}, ResponseMessages.LOGOUT_SUCCESS)
        
    except Exception as e:
        db.rollback()
        return create_error_response(ResponseMessages.OPERATION_FAILED)


# API [05] - 修改密码
@router.post("/password")
async def change_password(
    request: PasswordChangeRequest,
    current_user: User = Depends(get_current_user),
    db: Session = Depends(get_db)
) -> Dict[str, Any]:
    """
    API [05] - 用户更改自己的登录密码
    """
    try:
        # 验证旧密码
        if not auth_service.verify_password(request.old_password, current_user.password_hash):
            return create_error_response("旧密码不正确")
        
        # 更新密码
        new_password_hash = auth_service.hash_password(request.new_password)
        current_user.password_hash = new_password_hash
        
        db.commit()
        
        return create_success_response({}, ResponseMessages.PASSWORD_CHANGED)
        
    except Exception as e:
        db.rollback()
        return create_error_response(ResponseMessages.PASSWORD_CHANGE_FAILED)


# API [02] POST /auth/login
@router.post("/login")
async def login(
    request: Request,
    login_data: LoginRequest,
    db: Session = Depends(get_db)
) -> Dict[str, Any]:
    """验证用户和密码，生成session token"""
    try:
        # 用户认证
        user = AuthService.authenticate_user(db, login_data.email, login_data.password)
        
        if not user:
            # 记录失败的登录尝试
            await log_user_operation(
                db=db,
                current_user=None,
                operation=Operations.AUTH_LOGIN,
                details={"result": "failed", "email": login_data.email},
                request=request
            )
            return StandardResponse.error(ResponseMessages.LOGIN_FAILED)
        
        # 创建token
        token = AuthService.create_token(user.id, user.email)
        
        # 创建用户会话
        AuthService.create_user_session(db, user.id, token)
        
        # 获取用户权限
        permissions = AuthService.get_user_permissions(db, user.id)
        
        # 格式化用户数据
        user_data = AuthService.format_user_response(user, permissions)
        
        # 记录成功登录
        await log_user_operation(
            db=db,
            current_user=user,
            operation=Operations.AUTH_LOGIN,
            details={"result": "success", "email": user.email},
            request=request
        )
        
        return StandardResponse.login_success(user_data, token)
        
    except ValidationError as e:
        return StandardResponse.error(str(e))
    except Exception as e:
        return StandardResponse.error(ResponseMessages.LOGIN_FAILED)


# API [03] GET /auth/current
@router.get("/current")
async def get_current_user(
    user_and_permissions: tuple[User, list[str]] = Depends(get_current_user_with_permissions)
) -> Dict[str, Any]:
    """检测用户的token，查看当前的登陆状态，获取当前登陆的用户信息"""
    try:
        user, permissions = user_and_permissions
        user_data = AuthService.format_user_response(user, permissions)
        return StandardResponse.object_success(user_data)
    
    except AuthenticationError as e:
        return StandardResponse.error(str(e))


# API [04] POST /auth/logout
@router.post("/logout")
async def logout(
    request: Request,
    current_user: User = Depends(get_current_user),
    db: Session = Depends(get_db)
) -> Dict[str, Any]:
    """用户退出登陆，在服务器上删除登陆的token记录"""
    try:
        # 从Authorization头部获取token
        auth_header = request.headers.get("Authorization")
        if auth_header and auth_header.startswith("Bearer "):
            token = auth_header.split(" ")[1]
            
            # 使token失效
            success = AuthService.invalidate_user_session(db, token)
            
            # 记录登出操作
            await log_user_operation(
                db=db,
                current_user=current_user,
                operation=Operations.AUTH_LOGOUT,
                details={"result": "success" if success else "token_not_found"},
                request=request
            )
            
            return StandardResponse.success()
        
        return StandardResponse.error("Token not found in request")
    
    except Exception as e:
        return StandardResponse.error("Logout failed")


# API [05] POST /auth/password
@router.post("/password")
async def change_password(
    request: Request,
    password_data: PasswordChangeRequest,
    current_user: User = Depends(get_current_user),
    db: Session = Depends(get_db)
) -> Dict[str, Any]:
    """用户更改自己的登陆密码"""
    try:
        # 验证旧密码
        if not AuthService.verify_password(password_data.old_password, current_user.password_hash):
            return StandardResponse.error("Old password is incorrect")
        
        # 更新密码
        new_hash = AuthService.hash_password(password_data.new_password)
        current_user.password_hash = new_hash
        
        db.commit()
        
        # 记录密码修改操作
        await log_user_operation(
            db=db,
            current_user=current_user,
            operation=Operations.AUTH_PASSWORD_CHANGE,
            target_user_id=current_user.id,
            details={"result": "success"},
            request=request
        )
        
        return StandardResponse.success()
        
    except ValidationError as e:
        return StandardResponse.error(str(e))
    except Exception as e:
        return StandardResponse.error("Password update failed")


# 初始化函数，用于启动时调用
async def initialize_auth_system(db: Session):
    """初始化认证系统"""
    # 初始化默认权限
    await initialize_default_permissions(db)
    
    # 创建默认超级管理员
    await create_default_superuser(db)