"""
投资组合服务
"""
from typing import List, Optional
from sqlalchemy.orm import Session
from fastapi import HTTPException, status
from app.repositories.portfolios import PortfolioRepository
from app.repositories.teams import TeamRepository
from app.models.portfolio import Portfolio
from app.schemas.portfolios import PortfolioCreate, PortfolioTeamUpdate
import logging

logger = logging.getLogger(__name__)

class PortfolioService:
    """投资组合服务"""
    
    def __init__(self, db: Session):
        self.db = db
        self.portfolio_repo = PortfolioRepository(db)
        self.team_repo = TeamRepository(db)
    
    def create_portfolio(self, portfolio_create: PortfolioCreate) -> Portfolio:
        """创建投资组合"""
        # 检查基金名称是否已存在
        existing_portfolio = self.portfolio_repo.get_by_fund_name(portfolio_create.fund_name)
        if existing_portfolio:
            raise HTTPException(
                status_code=status.HTTP_400_BAD_REQUEST,
                detail="Fund name already exists"
            )
        
        # 检查账户名称是否已存在
        existing_account = self.portfolio_repo.get_by_account_name(portfolio_create.account_name)
        if existing_account:
            raise HTTPException(
                status_code=status.HTTP_400_BAD_REQUEST,
                detail="Account name already exists"
            )
        
        # 检查Ceffu钱包ID是否已存在
        existing_wallet = self.portfolio_repo.get_by_ceffu_wallet_id(portfolio_create.ceffu_wallet_id)
        if existing_wallet:
            raise HTTPException(
                status_code=status.HTTP_400_BAD_REQUEST,
                detail="Ceffu wallet ID already exists"
            )
        
        # 如果指定了团队ID，验证团队是否存在
        if portfolio_create.team_id:
            team = self.team_repo.get(portfolio_create.team_id)
            if not team:
                raise HTTPException(
                    status_code=status.HTTP_400_BAD_REQUEST,
                    detail="Team not found"
                )
        
        # 如果指定了父级投资组合，验证是否存在
        if portfolio_create.parent_id:
            parent = self.portfolio_repo.get(portfolio_create.parent_id)
            if not parent:
                raise HTTPException(
                    status_code=status.HTTP_400_BAD_REQUEST,
                    detail="Parent portfolio not found"
                )
        
        portfolio = self.portfolio_repo.create_portfolio(
            fund_name=portfolio_create.fund_name,
            fund_alias=portfolio_create.fund_alias,
            inception_time=portfolio_create.inception_time,
            account_name=portfolio_create.account_name,
            account_alias=portfolio_create.account_alias,
            ceffu_wallet_id=portfolio_create.ceffu_wallet_id,
            ceffu_wallet_name=portfolio_create.ceffu_wallet_name,
            team_id=portfolio_create.team_id,
            parent_id=portfolio_create.parent_id
        )
        
        logger.info(f"Portfolio created: {portfolio.fund_name}")
        return portfolio
    
    def get_portfolios(
        self,
        team_id: Optional[int] = None,
        portfolio_ids: Optional[List[int]] = None,
        limit: int = 100,
        offset: int = 0
    ) -> tuple[List[Portfolio], int]:
        """获取投资组合列表"""
        filters = {}
        
        if team_id is not None:
            filters["team_id"] = team_id
        
        if portfolio_ids:
            filters["id"] = portfolio_ids
        
        return self.portfolio_repo.get_multi(
            limit=limit,
            offset=offset,
            order_by="created_at",
            order_desc=True,
            filters=filters
        )
    
    def get_portfolio_by_id(self, portfolio_id: int) -> Portfolio:
        """根据ID获取投资组合"""
        portfolio = self.portfolio_repo.get(portfolio_id)
        if not portfolio:
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail="Portfolio not found"
            )
        return portfolio
    
    def update_portfolio_team(self, portfolio_id: int, team_update: PortfolioTeamUpdate) -> Portfolio:
        """更新投资组合的团队"""
        portfolio = self.get_portfolio_by_id(portfolio_id)
        
        # 验证团队是否存在
        team = self.team_repo.get(team_update.team_id)
        if not team:
            raise HTTPException(
                status_code=status.HTTP_400_BAD_REQUEST,
                detail="Team not found"
            )
        
        updated_portfolio = self.portfolio_repo.update_team(portfolio, team_update.team_id)
        logger.info(f"Portfolio team updated: {portfolio.fund_name} -> Team {team.name}")
        return updated_portfolio
    
    def get_portfolios_by_team(self, team_id: int) -> List[Portfolio]:
        """获取指定团队的投资组合"""
        return self.portfolio_repo.get_by_team_id(team_id)
    
    def get_root_portfolios(self) -> List[Portfolio]:
        """获取根级投资组合"""
        return self.portfolio_repo.get_root_portfolios()
    
    def get_children_portfolios(self, parent_id: int) -> List[Portfolio]:
        """获取子投资组合"""
        return self.portfolio_repo.get_children_portfolios(parent_id)
    
    def sync_portfolio_from_onetoken(self, onetoken_data: dict) -> Portfolio:
        """从OneToken数据同步投资组合"""
        # 这里实现从OneToken API获取的数据同步到数据库
        # 暂时返回None，后续实现
        pass
    
    def sync_portfolio_from_ceffu(self, ceffu_data: dict) -> Portfolio:
        """从Ceffu数据同步投资组合"""
        # 这里实现从Ceffu API获取的数据同步到数据库
        # 暂时返回None，后续实现
        pass