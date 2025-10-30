"""
Pydantic schemas for request/response models
"""
from typing import Optional, List, Dict, Any
from datetime import datetime
from pydantic import BaseModel, EmailStr, validator, model_validator
from enum import Enum


# Enums
class PermissionType(str, Enum):
    USER = "user"
    TEAM = "team"
    PROFIT = "profit"
    PORTFOLIO = "portfolio"
    BLACKLIST = "blacklist"


class SnapshotType(str, Enum):
    HOURLY = "hourly"
    DAILY = "daily"
    WEEKLY = "weekly"
    MONTHLY = "monthly"


# Base schemas
class BaseResponse(BaseModel):
    success: bool = True
    message: str = "Success"


class PaginationParams(BaseModel):
    page: int = 1
    page_size: int = 20
    
    @validator('page')
    def page_must_be_positive(cls, v):
        if v < 1:
            raise ValueError('Page must be positive')
        return v
    
    @validator('page_size')
    def page_size_must_be_reasonable(cls, v):
        if v < 1 or v > 100:
            raise ValueError('Page size must be between 1 and 100')
        return v


class PaginationResponse(BaseModel):
    total: int
    page: int
    page_size: int
    total_pages: int


# User schemas
class UserCreate(BaseModel):
    email: EmailStr
    permissions: Optional[List[str]] = []  # 用户权限列表
    name: Optional[str] = None
    # password 由系统自动生成
    # is_super 默认为 False（不允许通过API创建超级管理员）


class UserUpdate(BaseModel):
    email: Optional[EmailStr] = None
    name: Optional[str] = None
    is_super: Optional[bool] = None
    is_active: Optional[bool] = None


class UserResponse(BaseModel):
    id: int
    email: str
    name: Optional[str] = None
    is_super: bool
    is_active: Optional[bool] = None
    created_at: datetime
    
    class Config:
        from_attributes = True


class UserLogin(BaseModel):
    email: EmailStr
    password: str


class Token(BaseModel):
    access_token: str
    token_type: str = "bearer"
    expires_in: int
    user: UserResponse


# Permission schemas
class PermissionResponse(BaseModel):
    id: str
    name: str
    description: str
    
    class Config:
        from_attributes = True


class UserPermissionResponse(BaseModel):
    user_id: int
    permission_id: str
    granted_at: datetime
    
    class Config:
        from_attributes = True


# Portfolio schemas
class PortfolioCreate(BaseModel):
    name: str
    description: Optional[str] = None
    allocation_percentage: float
    
    @validator('allocation_percentage')
    def validate_percentage(cls, v):
        if v < 0 or v > 100:
            raise ValueError('Allocation percentage must be between 0 and 100')
        return v


class PortfolioUpdate(BaseModel):
    name: Optional[str] = None
    description: Optional[str] = None
    allocation_percentage: Optional[float] = None
    
    @validator('allocation_percentage')
    def validate_percentage(cls, v):
        if v is not None and (v < 0 or v > 100):
            raise ValueError('Allocation percentage must be between 0 and 100')
        return v


class PortfolioResponse(BaseModel):
    id: int
    name: str
    description: Optional[str]
    allocation_percentage: float
    created_at: datetime
    updated_at: datetime
    
    class Config:
        from_attributes = True


class PortfolioTeamUpdate(BaseModel):
    """更新投资组合团队"""
    teamId: int  # 使用 teamId 按照前端规范


# Team schemas
class TeamCreate(BaseModel):
    name: str
    description: Optional[str] = None


class TeamUpdate(BaseModel):
    name: Optional[str] = None
    description: Optional[str] = None


class TeamResponse(BaseModel):
    id: int
    name: str
    description: Optional[str]
    created_at: datetime
    updated_at: datetime
    
    class Config:
        from_attributes = True


class TeamMemberCreate(BaseModel):
    user_id: int
    team_id: int


class TeamMemberResponse(BaseModel):
    user_id: int
    team_id: int
    user: UserResponse
    joined_at: datetime
    
    class Config:
        from_attributes = True


# Profit schemas
class ProfitCreate(BaseModel):
    team_id: int
    portfolio_id: int
    amount: float
    profit_date: datetime
    description: Optional[str] = None


class ProfitUpdate(BaseModel):
    amount: Optional[float] = None
    profit_date: Optional[datetime] = None
    description: Optional[str] = None


class ProfitResponse(BaseModel):
    id: int
    team_id: int
    portfolio_id: int
    amount: float
    profit_date: datetime
    description: Optional[str]
    created_at: datetime
    updated_at: datetime
    
    class Config:
        from_attributes = True


class ProfitAllocationResponse(BaseModel):
    id: int
    profit_id: int
    user_id: int
    amount: float
    percentage: float
    user: UserResponse
    created_at: datetime
    
    class Config:
        from_attributes = True


# Snapshot schemas
class SnapshotCreate(BaseModel):
    snapshot_type: SnapshotType
    snapshot_date: datetime
    portfolio_data: Dict[str, Any]
    total_value: float


class SnapshotResponse(BaseModel):
    id: int
    snapshot_type: str
    snapshot_date: datetime
    portfolio_data: Dict[str, Any]
    total_value: float
    created_at: datetime
    
    class Config:
        from_attributes = True


class NavSnapshotResponse(BaseModel):
    id: int
    portfolio_id: int
    snapshot_type: str
    snapshot_date: datetime
    nav_value: float
    total_assets: float
    total_liabilities: float
    asset_breakdown: Dict[str, Any]
    created_at: datetime
    
    class Config:
        from_attributes = True


class ExchangeRateSnapshotResponse(BaseModel):
    id: int
    base_currency: str
    target_currency: str
    exchange_rate: float
    snapshot_date: datetime
    snapshot_type: str
    source: str
    created_at: datetime
    
    class Config:
        from_attributes = True


class AssetSnapshotResponse(BaseModel):
    id: int
    wallet_id: str
    asset_symbol: str
    balance: float
    usd_value: float
    snapshot_date: datetime
    snapshot_type: str
    additional_data: Dict[str, Any]
    created_at: datetime
    
    class Config:
        from_attributes = True


# Blacklist schemas
class BlacklistCreate(BaseModel):
    wallet_address: str
    reason: str
    notes: Optional[str] = None


class BlacklistUpdate(BaseModel):
    reason: Optional[str] = None
    notes: Optional[str] = None
    is_active: Optional[bool] = None


class BlacklistResponse(BaseModel):
    id: int
    wallet_address: str
    reason: str
    notes: Optional[str]
    is_active: bool
    created_at: datetime
    updated_at: datetime
    
    class Config:
        from_attributes = True


# Wallet schemas
class WalletBalanceResponse(BaseModel):
    wallet_id: str
    wallet_name: str
    total_balance: float
    assets: Dict[str, float]
    last_updated: datetime


class WalletTransactionResponse(BaseModel):
    id: str
    wallet_id: str
    transaction_type: str
    amount: float
    asset: str
    timestamp: datetime
    status: str
    hash: Optional[str] = None


# Dashboard schemas
class DashboardStatsResponse(BaseModel):
    total_portfolios: int
    total_users: int
    total_teams: int
    total_value: float
    daily_pnl: float
    weekly_pnl: float
    monthly_pnl: float


class AssetAllocationResponse(BaseModel):
    asset: str
    amount: float
    percentage: float
    usd_value: float


# API Response wrappers
class DataResponse(BaseResponse):
    data: Any


class ListResponse(BaseResponse):
    data: List[Any]
    pagination: PaginationResponse


# ==================== 收益管理相关Schema ====================

# 收益分配比例
class ProfitAllocationRatioCreate(BaseModel):
    portfolio_id: int
    to_team: int  # 10000 = 100%
    to_platform: int
    to_user: int


class ProfitAllocationRatioResponse(BaseModel):
    id: int
    portfolio_id: int
    version: int
    to_team: int
    to_platform: int
    to_user: int
    created_at: datetime
    created_by: int


# 收益提取
class ProfitWithdrawalCreate(BaseModel):
    from_type: str  # "team" or "platform"
    team_id: Optional[int] = None
    chain_id: str
    transaction_hash: str
    transaction_time: datetime
    usd_value: float
    assets: str  # "USDT" or "USDC"
    assets_amount: float


class ProfitWithdrawalResponse(BaseModel):
    id: int
    from_type: str
    team_id: Optional[int]
    chain_id: str
    transaction_hash: str
    transaction_time: datetime
    usd_value: float
    assets: str
    assets_amount: float
    created_at: datetime
    created_by: int


# 收益调账
class ProfitReallocationCreate(BaseModel):
    from_type: str  # "platform", "user", "team"
    to_type: str    # "user", "platform", "team"
    from_team_id: Optional[int] = None
    to_team_id: Optional[int] = None
    usd_value: float
    reason: str


class ProfitReallocationResponse(BaseModel):
    id: int
    from_type: str
    to_type: str
    from_team_id: Optional[int]
    to_team_id: Optional[int]
    usd_value: float
    reason: str
    created_at: datetime
    created_by: int


# ============================================================================
# 收益分配比例 (Profit Allocation Ratios)
# ============================================================================

class AllocationRatioBase(BaseModel):
    """
    分配比例基础模型 (0-100 百分比制)
    
    支持两种输入方式:
    1. 提供全部 3 个值 (自动验证总和 = 100)
    2. 提供任意 2 个值 (自动计算第 3 个)
    """
    toUser: Optional[int] = None       # 用户分成百分比 0-100
    toPlatform: Optional[int] = None   # 平台分成百分比 0-100
    toTeam: Optional[int] = None       # 团队分成百分比 0-100
    
    @validator('toUser', 'toPlatform', 'toTeam')
    def validate_range(cls, v):
        """验证每个值在 0-100 范围内"""
        if v is not None:
            if not isinstance(v, int):
                raise ValueError('分配比例必须是整数')
            if v < 0 or v > 100:
                raise ValueError('分配比例必须在 0-100 之间')
        return v
    
    @model_validator(mode='after')
    def calculate_missing_value(self):
        """
        自动计算缺失值或验证总和
        
        规则:
        1. 如果提供 3 个值: 验证总和是否 = 100
        2. 如果提供 2 个值: 自动计算第 3 个 = 100 - v1 - v2
        3. 如果提供 < 2 个值: 报错
        """
        to_user = self.toUser
        to_platform = self.toPlatform
        to_team = self.toTeam
        
        # 统计非空值数量
        provided_values = [v for v in [to_user, to_platform, to_team] if v is not None]
        non_null_count = len(provided_values)
        
        if non_null_count == 3:
            # 情况 1: 提供全部 3 个值，验证总和
            total = to_user + to_platform + to_team
            if total != 100:
                raise ValueError(
                    f'分配比例总和必须等于 100，当前总和为 {total} '
                    f'(用户: {to_user}, 平台: {to_platform}, 团队: {to_team})'
                )
        
        elif non_null_count == 2:
            # 情况 2: 提供 2 个值，自动计算第 3 个
            if to_user is None:
                calculated = 100 - to_platform - to_team
                if calculated < 0 or calculated > 100:
                    raise ValueError(
                        f'自动计算的用户分配比例 {calculated}% 超出范围 (0-100)，'
                        f'请检查输入: 平台 {to_platform}%, 团队 {to_team}%'
                    )
                self.toUser = calculated
            
            elif to_platform is None:
                calculated = 100 - to_user - to_team
                if calculated < 0 or calculated > 100:
                    raise ValueError(
                        f'自动计算的平台分配比例 {calculated}% 超出范围 (0-100)，'
                        f'请检查输入: 用户 {to_user}%, 团队 {to_team}%'
                    )
                self.toPlatform = calculated
            
            elif to_team is None:
                calculated = 100 - to_user - to_platform
                if calculated < 0 or calculated > 100:
                    raise ValueError(
                        f'自动计算的团队分配比例 {calculated}% 超出范围 (0-100)，'
                        f'请检查输入: 用户 {to_user}%, 平台 {to_platform}%'
                    )
                self.toTeam = calculated
        
        elif non_null_count == 1:
            # 情况 3: 只提供 1 个值，报错
            raise ValueError(
                '至少需要提供 2 个分配比例值 (第 3 个将自动计算)，'
                f'当前只提供了 {non_null_count} 个值'
            )
        
        else:
            # 情况 4: 未提供任何值，报错
            raise ValueError('必须至少提供 2 个分配比例值 (toUser, toPlatform, toTeam)')
        
        return self


class AllocationRatioCreate(AllocationRatioBase):
    """
    创建分配比例请求
    
    示例:
    1. 提供全部 3 个值:
       {"toUser": 50, "toPlatform": 30, "toTeam": 20}
    
    2. 提供 2 个值 (自动计算第 3 个):
       {"toUser": 50, "toPlatform": 30}  => toTeam = 20
       {"toUser": 50, "toTeam": 20}      => toPlatform = 30
       {"toPlatform": 30, "toTeam": 20}  => toUser = 50
    """
    portfolioId: int  # 投资组合 ID
    
    @validator('portfolioId')
    def validate_portfolio_id(cls, v):
        if v <= 0:
            raise ValueError('投资组合 ID 必须大于 0')
        return v


class AllocationRatioUpdate(AllocationRatioBase):
    """
    更新分配比例请求
    
    与创建请求相同的验证规则，但不需要 portfolioId
    """
    pass


class AllocationRatioResponse(BaseModel):
    """
    分配比例响应
    
    注意: 响应中总是返回完整的 3 个值 (即使创建时只提供了 2 个)
    """
    id: int
    portfolioId: int
    version: int                # 版本号，每次更新递增
    toUser: int                 # 用户分成百分比 0-100
    toPlatform: int             # 平台分成百分比 0-100
    toTeam: int                 # 团队分成百分比 0-100
    createdAt: datetime         # 创建时间
    createdBy: int              # 创建人 ID
    updatedAt: Optional[datetime] = None  # 更新时间
    updatedBy: Optional[int] = None       # 更新人 ID
    
    class Config:
        from_attributes = True
        
    @validator('toUser', 'toPlatform', 'toTeam')
    def validate_sum(cls, v, values):
        """验证总和 = 100 (响应数据完整性检查)"""
        # 注意: 这个验证只在所有字段都存在时执行
        if 'toUser' in values and 'toPlatform' in values:
            total = values['toUser'] + values['toPlatform'] + v
            if total != 100:
                raise ValueError(f'数据不一致: 分配比例总和为 {total}，应为 100')
        return v


class AllocationRatioListResponse(BaseModel):
    """
    分配比例列表响应
    """
    data: List[AllocationRatioResponse]
    pagination: PaginationResponse