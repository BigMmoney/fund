"""
收益相关模式
"""
from typing import Optional, Literal
from pydantic import BaseModel, Field, validator
from decimal import Decimal

class ProfitAllocationCreate(BaseModel):
    """创建收益分配比例模式"""
    portfolio_id: int = Field(..., alias="portffolioId")  # 保持与API文档一致的拼写错误
    allocation: dict = Field(...)
    
    @validator('allocation')
    def validate_allocation(cls, v):
        if not isinstance(v, dict):
            raise ValueError('allocation must be a dict')
        
        required_keys = ['toTeam', 'toPlatform', 'toUser']
        for key in required_keys:
            if key not in v:
                raise ValueError(f'allocation must contain {key}')
        
        total = v['toTeam'] + v['toPlatform'] + v['toUser']
        if total != 10000:
            raise ValueError('toTeam + toPlatform + toUser must equal 10000')
        
        return v

class ProfitAllocationResponse(BaseModel):
    """收益分配比例响应模式"""
    id: int
    portfolio_id: int
    version: int
    to_team_ratio: int
    to_platform_ratio: int
    to_user_ratio: int
    created_at: int
    created_by: int

    class Config:
        from_attributes = True

class AccProfitFromPortfolioResponse(BaseModel):
    """投资组合累计收益响应模式"""
    id: int
    portfolio_id: int
    snapshot_at: int
    acc_profit: str
    created_at: int

    class Config:
        from_attributes = True

class ProfitAllocationLogResponse(BaseModel):
    """收益分配日志响应模式"""
    id: int
    portfolio_id: int
    hour_end_at: int
    hourly_snapshot_prev: int
    hourly_snapshot_curr: int
    hourly_profit: str
    profit_to_team: str
    profit_to_user: str
    profit_to_platform: str
    allocation_ratio_id: int
    created_at: int

    class Config:
        from_attributes = True

class ProfitWithdrawalCreate(BaseModel):
    """创建提现记录模式"""
    from_type: Literal['team_portfolio', 'platform'] = Field(..., alias="from")
    portfolio_id: Optional[int] = None
    chain_id: str
    transaction_hash: str
    transaction_time: int
    usd_value: str
    assets: Literal['USDT', 'USDC', 'USD1']
    assets_amount: str

class ProfitWithdrawalResponse(BaseModel):
    """提现记录响应模式"""
    id: int
    from_type: str
    portfolio_id: Optional[int]
    chain_id: str
    transaction_hash: str
    transaction_time: int
    usd_value: str
    assets: str
    assets_amount: str
    created_at: int
    created_by: int

    class Config:
        from_attributes = True

class ProfitReallocationCreate(BaseModel):
    """创建调账记录模式"""
    from_type: Literal['platform', 'user', 'team_portfolio'] = Field(..., alias="from")
    to_type: Literal['platform', 'user', 'team_portfolio'] = Field(..., alias="to")
    from_portfolio_id: Optional[int] = None
    to_portfolio_id: Optional[int] = None
    usd_value: str
    reason: str = Field(..., alias="reasone")  # 保持与API文档一致的拼写错误

class ProfitReallocationResponse(BaseModel):
    """调账记录响应模式"""
    id: int
    from_type: str
    to_type: str
    from_portfolio_id: Optional[int]
    to_portfolio_id: Optional[int]
    usd_value: str
    reason: str
    created_at: int
    created_by: int

    class Config:
        from_attributes = True

class HourlyProfitUserResponse(BaseModel):
    """用户小时收益响应模式"""
    id: int
    hour_end_at: int
    profit_delta: str
    delta_from_fund: str
    delta_from_reallocation: str
    created_at: int

    class Config:
        from_attributes = True

class HourlyProfitPlatformResponse(BaseModel):
    """平台小时收益响应模式"""
    id: int
    hour_end_at: int
    profit_delta: str
    delta_from_fund: str
    delta_from_reallocation: str
    delta_from_withdraw: str
    created_at: int

    class Config:
        from_attributes = True

class HourlyProfitTeamResponse(BaseModel):
    """团队小时收益响应模式"""
    id: int
    portfolio_id: int
    hour_end_at: int
    profit_delta: str
    delta_from_fund: str
    delta_from_reallocation: str
    delta_from_withdraw: str
    created_at: int

    class Config:
        from_attributes = True

class AccProfitUserResponse(BaseModel):
    """用户累计收益响应模式"""
    id: int
    snapshot_at: int
    acc_profit: str
    created_at: int

    class Config:
        from_attributes = True

class AccProfitPlatformResponse(BaseModel):
    """平台累计收益响应模式"""
    id: int
    snapshot_at: int
    acc_profit: str
    created_at: int

    class Config:
        from_attributes = True

class AccProfitTeamResponse(BaseModel):
    """团队累计收益响应模式"""
    id: int
    portfolio_id: int
    snapshot_at: int
    acc_profit: str
    created_at: int

    class Config:
        from_attributes = True