"""
Services package - 业务服务和外部集成
"""

# 核心业务服务
from server.app.auth import AuthService
from server.app.users import UserService
from server.app.teams import TeamService
from server.app.portfolios import PortfolioService
from server.app.system import SystemService

# 外部集成服务
from server.app.ceffu_client import ceffu_client, get_portfolio_data, test_ceffu_integration
from server.app.scheduler import scheduler, start_scheduler, stop_scheduler, collect_snapshot_manually

__all__ = [
    # 核心业务服务
    "AuthService",
    "UserService", 
    "TeamService",
    "PortfolioService",
    "SystemService",
    
    # 外部集成服务
    "ceffu_client",
    "get_portfolio_data", 
    "test_ceffu_integration",
    "scheduler",
    "start_scheduler",
    "stop_scheduler",
    "collect_snapshot_manually"
]