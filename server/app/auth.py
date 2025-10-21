"""
JWT Bearer Token 认证系统
完全基于需求文档的认证规范实现
"""
import jwt
import bcrypt
from datetime import datetime, timedelta
from typing import Optional, Dict, Any
from fastapi import Depends, HTTPException, Request
from fastapi.security import HTTPBearer, HTTPAuthorizationCredentials
from sqlalchemy.orm import Session
from sqlalchemy import and_

from server.app.database import get_db
from server.app.models import User, UserSession, UserPermission, Permission
from server.app.responses import AuthenticationError, AuthorizationError


# JWT 配置
JWT_SECRET_KEY = "your-super-secret-jwt-key-change-in-production"  # 生产环境需要修改
JWT_ALGORITHM = "HS256"
JWT_EXPIRATION_HOURS = 24


# 便捷函数（向后兼容）
def get_password_hash(password: str) -> str:
    """获取密码哈希（兼容性函数）"""
    return AuthService.hash_password(password)


def verify_password(password: str, hashed: str) -> bool:
    """验证密码（兼容性函数）"""
    return AuthService.verify_password(password, hashed)


class AuthService:
    """认证服务"""
    
    @staticmethod
    def hash_password(password: str) -> str:
        """密码哈希"""
        salt = bcrypt.gensalt()
        return bcrypt.hashpw(password.encode('utf-8'), salt).decode('utf-8')
    
    @staticmethod
    def verify_password(password: str, hashed: str) -> bool:
        """验证密码"""
        return bcrypt.checkpw(password.encode('utf-8'), hashed.encode('utf-8'))
    
    @staticmethod
    def create_token(user_id: int, email: str) -> str:
        """创建JWT token"""
        now = datetime.utcnow()
        payload = {
            "user_id": user_id,
            "email": email,
            "iat": now,
            "exp": now + timedelta(hours=JWT_EXPIRATION_HOURS)
        }
        return jwt.encode(payload, JWT_SECRET_KEY, algorithm=JWT_ALGORITHM)
    
    @staticmethod
    def decode_token(token: str) -> Optional[Dict[str, Any]]:
        """解码JWT token"""
        try:
            payload = jwt.decode(token, JWT_SECRET_KEY, algorithms=[JWT_ALGORITHM])
            return payload
        except jwt.ExpiredSignatureError:
            return None
        except jwt.InvalidTokenError:
            return None
    
    @staticmethod
    def authenticate_user(db: Session, email: str, password: str) -> Optional[User]:
        """用户认证"""
        user = db.query(User).filter(User.email == email).first()
        if not user or user.suspended:
            return None
        
        if not AuthService.verify_password(password, user.password_hash):
            return None
        
        return user
    
    @staticmethod
    def create_user_session(db: Session, user_id: int, token: str) -> UserSession:
        """创建用户会话"""
        # 先清除该用户的旧会话
        db.query(UserSession).filter(UserSession.user_id == user_id).delete()
        
        # 创建新会话
        session = UserSession(
            user_id=user_id,
            token=token,
            expires_at=datetime.utcnow() + timedelta(hours=JWT_EXPIRATION_HOURS)
        )
        db.add(session)
        db.commit()
        return session
    
    @staticmethod
    def invalidate_user_session(db: Session, token: str) -> bool:
        """使用户会话失效"""
        session = db.query(UserSession).filter(UserSession.token == token).first()
        if session:
            db.delete(session)
            db.commit()
            return True
        return False
    
    @staticmethod
    def get_user_by_token(db: Session, token: str) -> Optional[User]:
        """通过token获取用户"""
        # 验证token格式
        payload = AuthService.decode_token(token)
        if not payload:
            return None
        
        # 检查数据库中的会话
        session = db.query(UserSession).filter(
            and_(
                UserSession.token == token,
                UserSession.expires_at > datetime.utcnow()
            )
        ).first()
        
        if not session:
            return None
        
        # 获取用户信息
        user = db.query(User).filter(User.id == session.user_id).first()
        if not user or user.suspended:
            return None
        
        return user
    
    @staticmethod
    def get_user_permissions(db: Session, user_id: int) -> list[str]:
        """获取用户权限列表"""
        permissions = db.query(Permission.id).join(
            UserPermission, Permission.id == UserPermission.permission_id
        ).filter(UserPermission.user_id == user_id).all()
        
        return [p.id for p in permissions]
    
    @staticmethod
    def format_user_response(user: User, permissions: list[str]) -> Dict[str, Any]:
        """格式化用户响应数据"""
        return {
            "id": user.id,
            "isSuper": user.is_super,
            "email": user.email,
            "permissions": permissions,
            "suspended": user.suspended,
            "updatedAt": int(user.updated_at.timestamp()),
            "createdAt": int(user.created_at.timestamp())
        }


# FastAPI 安全依赖
security = HTTPBearer()


async def get_current_user(
    credentials: HTTPAuthorizationCredentials = Depends(security),
    db: Session = Depends(get_db)
) -> User:
    """获取当前认证用户依赖"""
    if not credentials or not credentials.credentials:
        raise AuthenticationError("Authorization token required")
    
    user = AuthService.get_user_by_token(db, credentials.credentials)
    if not user:
        raise AuthenticationError("Invalid or expired token")
    
    return user


async def get_current_user_with_permissions(
    user: User = Depends(get_current_user),
    db: Session = Depends(get_db)
) -> tuple[User, list[str]]:
    """获取当前用户及其权限"""
    permissions = AuthService.get_user_permissions(db, user.id)
    return user, permissions


def require_permission(required_permission: str):
    """权限验证装饰器"""
    def permission_checker(
        user_and_permissions: tuple[User, list[str]] = Depends(get_current_user_with_permissions)
    ):
        user, permissions = user_and_permissions
        
        # 超级管理员拥有所有权限
        if user.is_super:
            return user
        
        # 检查具体权限
        if required_permission not in permissions:
            raise AuthorizationError(f"Permission '{required_permission}' required")
        
        return user
    
    return permission_checker


# 权限常量
class Permissions:
    USER = "user"
    TEAM = "team"
    PROFIT = "profit"
    PORTFOLIO = "portfolio"
    BLACKLIST = "blacklist"


# 初始化默认权限
async def initialize_default_permissions(db: Session):
    """初始化默认权限"""
    default_permissions = [
        {"id": "user", "label": "用户管理", "description": "管理系统用户和权限"},
        {"id": "team", "label": "团队管理", "description": "管理交易团队"},
        {"id": "profit", "label": "收益管理", "description": "管理收益分配和提现"},
        {"id": "portfolio", "label": "投资组合管理", "description": "管理投资组合配置"},
        {"id": "blacklist", "label": "黑名单管理", "description": "管理黑名单地址"}
    ]
    
    for perm_data in default_permissions:
        existing = db.query(Permission).filter(Permission.id == perm_data["id"]).first()
        if not existing:
            permission = Permission(**perm_data)
            db.add(permission)
    
    db.commit()


# 创建默认超级管理员
async def create_default_superuser(db: Session, email: str = "admin@example.com", password: str = "admin123"):
    """创建默认超级管理员"""
    existing_user = db.query(User).filter(User.email == email).first()
    if existing_user:
        return existing_user
    
    user = User(
        email=email,
        password_hash=AuthService.hash_password(password),
        is_super=True,
        suspended=False
    )
    db.add(user)
    db.commit()
    db.refresh(user)
    
    return user