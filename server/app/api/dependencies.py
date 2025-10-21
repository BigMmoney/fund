"""
Authentication dependencies for FastAPI
"""
from typing import Optional
from fastapi import Depends, HTTPException, status
from fastapi.security import HTTPBearer, HTTPAuthorizationCredentials
from sqlalchemy.orm import Session
from server.app.database import get_db
# from server.app.core.security import get_current_user_from_token, check_user_permission
from server.app.models import User

security = HTTPBearer()


# 临时简化实现 - 避免复杂依赖
def get_current_user_from_token(db: Session, token: str) -> Optional[User]:
    """从token获取当前用户（简化版）"""
    # TODO: 实现JWT验证
    return None


def check_user_permission(db: Session, user_id: int, permission: str) -> bool:
    """检查用户权限（简化版）"""
    # TODO: 实现权限检查
    return True


async def get_current_user(
    credentials: HTTPAuthorizationCredentials = Depends(security),
    db: Session = Depends(get_db)
) -> User:
    """Get current authenticated user"""
    token = credentials.credentials
    user = get_current_user_from_token(db, token)
    
    if not user:
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail="Invalid authentication credentials",
            headers={"WWW-Authenticate": "Bearer"},
        )
    
    return user


async def get_current_active_user(
    current_user: User = Depends(get_current_user)
) -> User:
    """Get current active user"""
    if not current_user.is_active:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Inactive user"
        )
    return current_user


def require_permission(permission: str):
    """Decorator to require specific permission"""
    def permission_dependency(
        current_user: User = Depends(get_current_active_user),
        db: Session = Depends(get_db)
    ):
        if not check_user_permission(db, current_user.id, permission):
            raise HTTPException(
                status_code=status.HTTP_403_FORBIDDEN,
                detail=f"Insufficient permissions. Required: {permission}"
            )
        return current_user
    
    return permission_dependency


def require_super_admin(
    current_user: User = Depends(get_current_active_user)
) -> User:
    """Require super admin permissions"""
    if not current_user.is_super:
        raise HTTPException(
            status_code=status.HTTP_403_FORBIDDEN,
            detail="Super admin access required"
        )
    return current_user


# Permission dependencies
require_user_permission = require_permission("user")
require_team_permission = require_permission("team") 
require_profit_permission = require_permission("profit")
require_portfolio_permission = require_permission("portfolio")
require_blacklist_permission = require_permission("blacklist")