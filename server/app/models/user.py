"""
用户模型
"""
from sqlalchemy import Column, Integer, String, Boolean, Text, DateTime, func
from sqlalchemy.ext.hybrid import hybrid_property
from .base import BaseModel
import json

class User(BaseModel):
    """用户模型"""
    __tablename__ = "users"
    
    email = Column(String(255), unique=True, nullable=False, index=True, comment="用户邮箱")
    password_hash = Column(String(255), nullable=False, comment="密码哈希")
    is_super = Column(Boolean, default=False, nullable=False, comment="是否为超级管理员")
    suspended = Column(Boolean, default=False, nullable=False, comment="是否被禁用")
    
    # 权限存储为JSON字符串
    permissions_json = Column(Text, nullable=False, default="[]", comment="权限JSON数组")
    
    # 最后登录时间
    last_login_at = Column(DateTime, nullable=True, comment="最后登录时间")
    
    @hybrid_property
    def permissions(self):
        """获取权限列表"""
        try:
            return json.loads(self.permissions_json or "[]")
        except (json.JSONDecodeError, TypeError):
            return []
    
    @permissions.setter
    def permissions(self, value):
        """设置权限列表"""
        if isinstance(value, list):
            self.permissions_json = json.dumps(value)
        else:
            self.permissions_json = "[]"
    
    def has_permission(self, permission: str) -> bool:
        """检查是否有指定权限"""
        if self.is_super:
            return True
        return permission in self.permissions
    
    def to_dict(self):
        """转换为字典"""
        data = super().to_dict()
        data['permissions'] = self.permissions
        # 移除敏感信息
        data.pop('password_hash', None)
        data.pop('permissions_json', None)
        return data
