"""
基础Repository类
"""
from typing import List, Optional, Dict, Any, TypeVar, Generic
from sqlalchemy.orm import Session
from sqlalchemy import desc, asc
from app.models.base import BaseModel

T = TypeVar('T', bound=BaseModel)

class BaseRepository(Generic[T]):
    """基础Repository类"""
    
    def __init__(self, model: type[T], db: Session):
        self.model = model
        self.db = db
    
    def create(self, obj_in: Dict[str, Any]) -> T:
        """创建对象"""
        db_obj = self.model(**obj_in)
        self.db.add(db_obj)
        self.db.commit()
        self.db.refresh(db_obj)
        return db_obj
    
    def get(self, id: int) -> Optional[T]:
        """根据ID获取对象"""
        return self.db.query(self.model).filter(self.model.id == id).first()
    
    def get_multi(
        self, 
        limit: int = 100, 
        offset: int = 0,
        order_by: str = "id",
        order_desc: bool = True,
        filters: Dict[str, Any] = None
    ) -> tuple[List[T], int]:
        """获取多个对象"""
        query = self.db.query(self.model)
        
        # 应用过滤器
        if filters:
            for key, value in filters.items():
                if hasattr(self.model, key) and value is not None:
                    if isinstance(value, list):
                        query = query.filter(getattr(self.model, key).in_(value))
                    else:
                        query = query.filter(getattr(self.model, key) == value)
        
        # 计算总数
        total = query.count()
        
        # 排序
        if hasattr(self.model, order_by):
            order_column = getattr(self.model, order_by)
            if order_desc:
                query = query.order_by(desc(order_column))
            else:
                query = query.order_by(asc(order_column))
        
        # 分页
        items = query.offset(offset).limit(limit).all()
        
        return items, total
    
    def update(self, db_obj: T, obj_in: Dict[str, Any]) -> T:
        """更新对象"""
        for field, value in obj_in.items():
            if hasattr(db_obj, field):
                setattr(db_obj, field, value)
        
        self.db.commit()
        self.db.refresh(db_obj)
        return db_obj
    
    def delete(self, id: int) -> bool:
        """删除对象"""
        db_obj = self.get(id)
        if db_obj:
            self.db.delete(db_obj)
            self.db.commit()
            return True
        return False
    
    def count(self, filters: Dict[str, Any] = None) -> int:
        """计数"""
        query = self.db.query(self.model)
        
        if filters:
            for key, value in filters.items():
                if hasattr(self.model, key) and value is not None:
                    if isinstance(value, list):
                        query = query.filter(getattr(self.model, key).in_(value))
                    else:
                        query = query.filter(getattr(self.model, key) == value)
        
        return query.count()
    
    def exists(self, filters: Dict[str, Any]) -> bool:
        """检查是否存在"""
        query = self.db.query(self.model)
        
        for key, value in filters.items():
            if hasattr(self.model, key):
                query = query.filter(getattr(self.model, key) == value)
        
        return query.first() is not None