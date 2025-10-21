"""
团队模型
"""
from sqlalchemy import Column, Integer, String, ForeignKey
from sqlalchemy.orm import relationship
from .base import BaseModel

class Team(BaseModel):
    """交易团队模型"""
    __tablename__ = "teams"
    
    name = Column(String(255), nullable=False, comment="团队名称")
    created_by = Column(Integer, ForeignKey("users.id"), nullable=False, comment="创建者用户ID")
    
    # 关系
    creator = relationship("User", foreign_keys=[created_by])
    portfolios = relationship("Portfolio", back_populates="team")
