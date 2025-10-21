"""
投资组合相关模式
"""
from typing import Optional
from pydantic import BaseModel

class PortfolioBase(BaseModel):
    """投资组合基础模式"""
    fund_name: str
    fund_alias: str
    inception_time: int
    account_name: str
    account_alias: str
    ceffu_wallet_id: str
    ceffu_wallet_name: str

class PortfolioCreate(PortfolioBase):
    """创建投资组合模式"""
    team_id: Optional[int] = None
    parent_id: Optional[int] = None

class PortfolioTeamUpdate(BaseModel):
    """更新投资组合团队模式"""
    teamId: int  # 注意：按照需求文档使用 teamId 而不是 team_id

class PortfolioResponse(BaseModel):
    """投资组合响应模式"""
    id: int
    fund_name: str
    fund_alias: str
    inception_time: int
    account_name: str
    account_alias: str
    ceffu_wallet_id: str
    ceffu_wallet_name: str
    team_id: Optional[int] = None
    parent_id: Optional[int] = None

    class Config:
        from_attributes = True