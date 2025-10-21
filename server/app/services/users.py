"""
用户服务
"""
from typing import List, Optional, Dict, Any
from sqlalchemy.orm import Session
from fastapi import HTTPException, status
from app.repositories.users import UserRepository
from app.repositories.permissions import PermissionRepository
from app.models.user import User
from app.schemas.users import UserCreate, UserUpdate, UserPasswordUpdate
from app.core.security import security_manager
import logging

logger = logging.getLogger(__name__)

class UserService:
    """用户服务"""
    
    def __init__(self, db: Session):
        self.db = db
        self.user_repo = UserRepository(db)
        self.permission_repo = PermissionRepository(db)
    
    def authenticate_user(self, email: str, password: str) -> Optional[User]:
        """用户认证"""
        user = self.user_repo.get_by_email(email)
        if not user:
            return None
        
        if user.suspended:
            raise HTTPException(
                status_code=status.HTTP_403_FORBIDDEN,
                detail="User account is suspended"
            )
        
        if not self.user_repo.verify_password(user, password):
            return None
        
        # 更新最后登录时间
        self.user_repo.update(user, {"last_login_at": int(time.time())})
        
        return user
    
    def create_user(self, user_create: UserCreate) -> User:
        """创建用户"""
        # 检查邮箱是否已存在
        existing_user = self.user_repo.get_by_email(user_create.email)
        if existing_user:
            raise HTTPException(
                status_code=status.HTTP_400_BAD_REQUEST,
                detail="Email already registered"
            )
        
        # 验证权限是否有效
        self._validate_permissions(user_create.permissions)
        
        # 创建用户（使用默认密码）
        default_password = "123456"
        user = self.user_repo.create_user(
            email=user_create.email,
            password=default_password,
            permissions=user_create.permissions
        )
        
        logger.info(f"User created: {user.email}")
        return user
    
    def get_all_users(self) -> List[User]:
        """获取所有用户"""
        users, _ = self.user_repo.get_multi(limit=1000)
        return users
    
    def get_user_by_id(self, user_id: int) -> User:
        """根据ID获取用户"""
        user = self.user_repo.get(user_id)
        if not user:
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail="User not found"
            )
        return user
    
    def update_user_permissions(self, user_id: int, permissions: List[str]) -> User:
        """更新用户权限"""
        user = self.get_user_by_id(user_id)
        
        if user.is_super:
            raise HTTPException(
                status_code=status.HTTP_400_BAD_REQUEST,
                detail="Cannot modify super admin permissions"
            )
        
        # 验证权限是否有效
        self._validate_permissions(permissions)
        
        updated_user = self.user_repo.update_permissions(user, permissions)
        logger.info(f"User permissions updated: {user.email}")
        return updated_user
    
    def reset_user_password(self, user_id: int) -> str:
        """重置用户密码"""
        user = self.get_user_by_id(user_id)
        
        default_password = self.user_repo.reset_password(user)
        logger.info(f"Password reset for user: {user.email}")
        return default_password
    
    def change_user_password(self, user: User, password_update: UserPasswordUpdate) -> None:
        """用户修改密码"""
        # 验证旧密码
        if not self.user_repo.verify_password(user, password_update.old_password):
            raise HTTPException(
                status_code=status.HTTP_400_BAD_REQUEST,
                detail="Invalid old password"
            )
        
        # 验证新密码强度
        password_errors = security_manager.validate_password_strength(password_update.new_password)
        if password_errors:
            raise HTTPException(
                status_code=status.HTTP_400_BAD_REQUEST,
                detail=f"Password validation failed: {'; '.join(password_errors)}"
            )
        
        # 更新密码
        self.user_repo.update_password(user, password_update.new_password)
        logger.info(f"Password changed for user: {user.email}")
    
    def suspend_user(self, user_id: int) -> User:
        """禁用用户"""
        user = self.get_user_by_id(user_id)
        
        if user.is_super:
            raise HTTPException(
                status_code=status.HTTP_400_BAD_REQUEST,
                detail="Cannot suspend super admin"
            )
        
        updated_user = self.user_repo.suspend_user(user)
        logger.info(f"User suspended: {user.email}")
        return updated_user
    
    def activate_user(self, user_id: int) -> User:
        """激活用户"""
        user = self.get_user_by_id(user_id)
        updated_user = self.user_repo.activate_user(user)
        logger.info(f"User activated: {user.email}")
        return updated_user
    
    def _validate_permissions(self, permissions: List[str]) -> None:
        """验证权限是否有效"""
        valid_permissions = [perm.id for perm in self.permission_repo.get_all_permissions()]
        
        for permission in permissions:
            if permission not in valid_permissions:
                raise HTTPException(
                    status_code=status.HTTP_400_BAD_REQUEST,
                    detail=f"Invalid permission: {permission}"
                )
    
    def check_user_permission(self, user: User, required_permission: str) -> bool:
        """检查用户是否有指定权限"""
        return user.has_permission(required_permission)