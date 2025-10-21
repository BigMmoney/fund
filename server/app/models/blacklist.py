"""
黑名单模?
"""
from sqlalchemy import Column, String, Text
from .base import BaseModel

class Blacklist(BaseModel):
    """黑名单地址模型"""
    __tablename__ = "blacklist"
    
    address = Column(String(255), unique=True, nullable=False, index=True, comment="黑名单地址（小写）")
    note = Column(Text, nullable=True, comment="备注")
