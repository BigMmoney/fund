"""
用户Repository
"""
from typing import Optional, List
from sqlalchemy.orm import Session
from sqlalchemy import or_
from app.models.user import User
from app.core.security import security_manager
from server.app.base import BaseRepository

class UserRepository(BaseRepository[User]):
    """用户Repository"""
    
    def __init__(self, db: Session):
        super().__init__(User, db)
    
    def get_by_email(self, email: str) -> Optional[User]:
        """根据邮箱获取用户"""
        return self.db.query(User).filter(User.email == email).first()
    
    def create_user(self, email: str, password: str, permissions: List[str] = None, is_super: bool = False) -> User:
        """创建用户"""
        password_hash = security_manager.hash_password(password)
        user_data = {
            "email": email,
            "password_hash": password_hash,
            "is_super": is_super,
            "permissions": permissions or []
        }
        return self.create(user_data)
    
    def verify_password(self, user: User, password: str) -> bool:
        """验证密码"""
        return security_manager.verify_password(password, user.password_hash)
    
    def update_password(self, user: User, new_password: str) -> User:
        """更新密码"""
        password_hash = security_manager.hash_password(new_password)
        return self.update(user, {"password_hash": password_hash})
    
    def reset_password(self, user: User) -> str:
        """重置密码为默认密码"""
        default_password = "123456"  # 或者生成随机密码
        password_hash = security_manager.hash_password(default_password)
        self.update(user, {"password_hash": password_hash})
        return default_password
    
    def update_permissions(self, user: User, permissions: List[str]) -> User:
        """更新用户权限"""
        return self.update(user, {"permissions": permissions})
    
    def suspend_user(self, user: User) -> User:
        """禁用用户"""
        return self.update(user, {"suspended": True})
    
    def activate_user(self, user: User) -> User:
        """激活用户"""
        return self.update(user, {"suspended": False})
    
    def search_users(self, query: str) -> List[User]:
        """搜索用户"""
        return (
            self.db.query(User)
            .filter(or_(
                User.email.contains(query),
                User.permissions_json.contains(query)
            ))
            .all()
        )