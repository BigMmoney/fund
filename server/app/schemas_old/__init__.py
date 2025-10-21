"""
模式定义模块
"""
from server.app.responses import (
    BaseResponse,
    ListResponse,
    ObjectResponse,
    TokenResponse,
    SuccessResponse,
    ErrorResponse,
    ListDataResponse,
    ObjectDataResponse,
    LoginDataResponse
)
from server.app.users import (
    UserBase,
    UserCreate,
    UserUpdate,
    UserPasswordUpdate,
    UserPasswordReset,
    UserSuspend,
    UserResponse,
    LoginRequest,
    LoginResponse
)
from server.app.teams import (
    TeamBase,
    TeamCreate,
    TeamUpdate,
    TeamResponse
)
from server.app.portfolios import (
    PortfolioBase,
    PortfolioCreate,
    PortfolioTeamUpdate,
    PortfolioResponse
)
from server.app.permissions import PermissionResponse
from server.app.snapshots import (
    NavSnapshotResponse,
    RateSnapshotResponse,
    AssetsSnapshotResponse
)
from server.app.profits import (
    ProfitAllocationCreate,
    ProfitAllocationResponse,
    AccProfitFromPortfolioResponse,
    ProfitAllocationLogResponse,
    ProfitWithdrawalCreate,
    ProfitWithdrawalResponse,
    ProfitReallocationCreate,
    ProfitReallocationResponse,
    HourlyProfitUserResponse,
    HourlyProfitPlatformResponse,
    HourlyProfitTeamResponse,
    AccProfitUserResponse,
    AccProfitPlatformResponse,
    AccProfitTeamResponse
)
from server.app.blacklist import (
    BlacklistCreate,
    BlacklistResponse
)
from server.app.system import SystemStatusResponse

__all__ = [
    # 响应基础类型
    "BaseResponse",
    "ListResponse", 
    "ObjectResponse",
    "TokenResponse",
    "SuccessResponse",
    "ErrorResponse",
    "ListDataResponse",
    "ObjectDataResponse",
    "LoginDataResponse",
    
    # 用户模式
    "UserBase",
    "UserCreate",
    "UserUpdate", 
    "UserPasswordUpdate",
    "UserPasswordReset",
    "UserSuspend",
    "UserResponse",
    "LoginRequest",
    "LoginResponse",
    
    # 团队模式
    "TeamBase",
    "TeamCreate",
    "TeamUpdate",
    "TeamResponse",
    
    # 投资组合模式
    "PortfolioBase",
    "PortfolioCreate",
    "PortfolioTeamUpdate",
    "PortfolioResponse",
    
    # 权限模式
    "PermissionResponse",
    
    # 快照模式
    "NavSnapshotResponse",
    "RateSnapshotResponse",
    "AssetsSnapshotResponse",
    
    # 收益模式
    "ProfitAllocationCreate",
    "ProfitAllocationResponse",
    "AccProfitFromPortfolioResponse",
    "ProfitAllocationLogResponse",
    "ProfitWithdrawalCreate",
    "ProfitWithdrawalResponse",
    "ProfitReallocationCreate",
    "ProfitReallocationResponse",
    "HourlyProfitUserResponse",
    "HourlyProfitPlatformResponse",
    "HourlyProfitTeamResponse",
    "AccProfitUserResponse",
    "AccProfitPlatformResponse",
    "AccProfitTeamResponse",
    
    # 黑名单模式
    "BlacklistCreate",
    "BlacklistResponse",
    
    # 系统模式
    "SystemStatusResponse"
]