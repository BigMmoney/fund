"""
认证服务
"""
from typing import Optional
from sqlalchemy.orm import Session
from fastapi import HTTPException, status
from app.models.user import User
from app.repositories.users import UserRepository
from app.core.security import security_manager
from app.schemas.users import LoginRequest
import time
import logging

logger = logging.getLogger(__name__)

class AuthService:
    """认证服务"""
    
    def __init__(self, db: Session):
        self.db = db
        self.user_repo = UserRepository(db)
    
    def login(self, login_request: LoginRequest) -> tuple[User, str]:
        """用户登录"""
        user = self.user_repo.get_by_email(login_request.email)
        if not user:
            raise HTTPException(
                status_code=status.HTTP_401_UNAUTHORIZED,
                detail="Login Failed"
            )
        
        if user.suspended:
            raise HTTPException(
                status_code=status.HTTP_403_FORBIDDEN,
                detail="User account is suspended"
            )
        
        if not self.user_repo.verify_password(user, login_request.password):
            raise HTTPException(
                status_code=status.HTTP_401_UNAUTHORIZED,
                detail="Login Failed"
            )
        
        # 更新最后登录时间
        self.user_repo.update(user, {"last_login_at": int(time.time())})
        
        # 生成访问令牌
        token_data = {
            "sub": str(user.id),
            "email": user.email,
            "permissions": user.permissions,
            "is_super": user.is_super
        }
        access_token = security_manager.create_access_token(token_data)
        
        logger.info(f"User logged in: {user.email}")
        return user, access_token
    
    def get_current_user(self, token: str) -> User:
        """根据token获取当前用户"""
        try:
            payload = security_manager.verify_token(token)
            if payload is None:
                raise HTTPException(
                    status_code=status.HTTP_401_UNAUTHORIZED,
                    detail="Invalid authentication credentials"
                )
            
            user_id = int(payload.get("sub"))
            user = self.user_repo.get(user_id)
            if user is None:
                raise HTTPException(
                    status_code=status.HTTP_401_UNAUTHORIZED,
                    detail="User not found"
                )
            
            if user.suspended:
                raise HTTPException(
                    status_code=status.HTTP_403_FORBIDDEN,
                    detail="User account is suspended"
                )
            
            return user
            
        except ValueError:
            raise HTTPException(
                status_code=status.HTTP_401_UNAUTHORIZED,
                detail="Invalid authentication credentials"
            )
    
    def logout(self, token: str) -> bool:
        """用户登出"""
        # 在实际实现中，这里可以将token加入黑名单
        # 或者使用Redis等存储失效的token
        # 当前简化实现，总是返回成功
        logger.info("User logged out")
        return True
    
    def verify_permission(self, user: User, required_permission: str) -> bool:
        """验证用户权限"""
        if user.suspended:
            return False
        
        return user.has_permission(required_permission)