"""
数据库模型模块
"""
from .base import Base
from .user import User
from .team import Team
from .portfolio import Portfolio
from .permission import Permission
from .snapshots import NavSnapshot, RateSnapshot, AssetsSnapshot
from .profit import (
    AccProfitFromPortfolio,
    ProfitAllocationRatio,
    ProfitAllocationLog,
    ProfitReallocation,
    ProfitWithdrawal,
    HourlyProfitUser,
    HourlyProfitPlatform,
    HourlyProfitTeam,
    AccProfitUser,
    AccProfitPlatform,
    AccProfitTeam
)
from .blacklist import Blacklist

# 从 models.py 导入认证相关模型
try:
    from server.app.models import UserSession, UserPermission
except ImportError:
    # 如果 models.py 不存在这些模型，跳过
    UserSession = None
    UserPermission = None

__all__ = [
    "Base",
    "User",
    "Team", 
    "Portfolio",
    "Permission",
    "NavSnapshot",
    "RateSnapshot",
    "AssetsSnapshot",
    "AccProfitFromPortfolio",
    "ProfitAllocationRatio",
    "ProfitAllocationLog",
    "ProfitReallocation",
    "ProfitWithdrawal",
    "HourlyProfitUser",
    "HourlyProfitPlatform", 
    "HourlyProfitTeam",
    "AccProfitUser",
    "AccProfitPlatform",
    "AccProfitTeam",
    "Blacklist",
    "UserSession",
    "UserPermission",
]