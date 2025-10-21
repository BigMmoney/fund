"""
团队相关模式
"""
from pydantic import BaseModel

class TeamBase(BaseModel):
    """团队基础模式"""
    name: str

class TeamCreate(TeamBase):
    """创建团队模式"""
    pass

class TeamUpdate(TeamBase):
    """更新团队模式"""
    pass

class TeamResponse(BaseModel):
    """团队响应模式"""
    id: int
    name: str
    created_at: int
    created_by: int

    class Config:
        from_attributes = True