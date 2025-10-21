"""
权限Repository
"""
from typing import List
from sqlalchemy.orm import Session
from app.models.permission import Permission
from server.app.base import BaseRepository

class PermissionRepository(BaseRepository[Permission]):
    """权限Repository"""
    
    def __init__(self, db: Session):
        super().__init__(Permission, db)
    
    def get_all_permissions(self) -> List[Permission]:
        """获取所有权限"""
        return self.db.query(Permission).all()
    
    def create_permission(self, id: str, label: str, description: str = None) -> Permission:
        """创建权限"""
        permission_data = {
            "id": id,
            "label": label,
            "description": description
        }
        return self.create(permission_data)
    
    def init_default_permissions(self):
        """初始化默认权限"""
        default_permissions = [
            {"id": "user", "label": "用户管理", "description": "管理系统用户"},
            {"id": "team", "label": "团队管理", "description": "管理交易团队"},
            {"id": "profit", "label": "收益管理", "description": "管理收益分配和提现"},
            {"id": "portfolio", "label": "投资组合管理", "description": "管理投资组合"},
            {"id": "blacklist", "label": "黑名单管理", "description": "管理黑名单地址"}
        ]
        
        for perm in default_permissions:
            existing = self.db.query(Permission).filter(Permission.id == perm["id"]).first()
            if not existing:
                self.create_permission(**perm)