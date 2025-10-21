"""
投资组合Repository
"""
from typing import List, Optional, Dict, Any
from sqlalchemy.orm import Session
from sqlalchemy import and_
from app.models.portfolio import Portfolio
from server.app.base import BaseRepository

class PortfolioRepository(BaseRepository[Portfolio]):
    """投资组合Repository"""
    
    def __init__(self, db: Session):
        super().__init__(Portfolio, db)
    
    def create_portfolio(
        self,
        fund_name: str,
        fund_alias: str,
        inception_time: int,
        account_name: str,
        account_alias: str,
        ceffu_wallet_id: str,
        ceffu_wallet_name: str,
        team_id: Optional[int] = None,
        parent_id: Optional[int] = None
    ) -> Portfolio:
        """创建投资组合"""
        portfolio_data = {
            "fund_name": fund_name,
            "fund_alias": fund_alias,
            "inception_time": inception_time,
            "account_name": account_name,
            "account_alias": account_alias,
            "ceffu_wallet_id": ceffu_wallet_id,
            "ceffu_wallet_name": ceffu_wallet_name,
            "team_id": team_id,
            "parent_id": parent_id
        }
        return self.create(portfolio_data)
    
    def get_by_fund_name(self, fund_name: str) -> Optional[Portfolio]:
        """根据基金名称获取投资组合"""
        return self.db.query(Portfolio).filter(Portfolio.fund_name == fund_name).first()
    
    def get_by_team_id(self, team_id: int) -> List[Portfolio]:
        """获取指定团队的投资组合"""
        return self.db.query(Portfolio).filter(Portfolio.team_id == team_id).all()
    
    def get_by_account_name(self, account_name: str) -> Optional[Portfolio]:
        """根据账户名称获取投资组合"""
        return self.db.query(Portfolio).filter(Portfolio.account_name == account_name).first()
    
    def get_by_ceffu_wallet_id(self, ceffu_wallet_id: str) -> Optional[Portfolio]:
        """根据Ceffu钱包ID获取投资组合"""
        return self.db.query(Portfolio).filter(Portfolio.ceffu_wallet_id == ceffu_wallet_id).first()
    
    def update_team(self, portfolio: Portfolio, team_id: int) -> Portfolio:
        """更新投资组合的团队"""
        return self.update(portfolio, {"team_id": team_id})
    
    def get_root_portfolios(self) -> List[Portfolio]:
        """获取根级投资组合（没有父级的）"""
        return self.db.query(Portfolio).filter(Portfolio.parent_id.is_(None)).all()
    
    def get_children_portfolios(self, parent_id: int) -> List[Portfolio]:
        """获取子投资组合"""
        return self.db.query(Portfolio).filter(Portfolio.parent_id == parent_id).all()
    
    def search_portfolios(
        self,
        team_id: Optional[int] = None,
        fund_name: Optional[str] = None,
        account_name: Optional[str] = None,
        limit: int = 100,
        offset: int = 0
    ) -> tuple[List[Portfolio], int]:
        """搜索投资组合"""
        filters = {}
        if team_id is not None:
            filters["team_id"] = team_id
        if fund_name:
            filters["fund_name"] = fund_name
        if account_name:
            filters["account_name"] = account_name
        
        return self.get_multi(
            limit=limit,
            offset=offset,
            order_by="created_at",
            order_desc=True,
            filters=filters
        )