"""
收益数据Repository
"""
from typing import List, Optional
from sqlalchemy.orm import Session
from sqlalchemy import and_, desc
from decimal import Decimal
from app.models.profit import (
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
from server.app.base import BaseRepository

class AccProfitFromPortfolioRepository(BaseRepository[AccProfitFromPortfolio]):
    """投资组合累计收益Repository"""
    
    def __init__(self, db: Session):
        super().__init__(AccProfitFromPortfolio, db)
    
    def create_snapshot(self, portfolio_id: int, snapshot_at: int, acc_profit: Decimal) -> AccProfitFromPortfolio:
        """创建累计收益快照"""
        data = {
            "portfolio_id": portfolio_id,
            "snapshot_at": snapshot_at,
            "acc_profit": acc_profit
        }
        return self.create(data)
    
    def get_by_time_range(
        self,
        portfolio_id: Optional[int] = None,
        snapshot_at_gte: Optional[int] = None,
        snapshot_at_lt: Optional[int] = None,
        snapshot_at_list: Optional[List[int]] = None,
        limit: int = 100,
        offset: int = 0
    ) -> tuple[List[AccProfitFromPortfolio], int]:
        """按时间范围获取快照"""
        query = self.db.query(AccProfitFromPortfolio)
        
        if portfolio_id is not None:
            query = query.filter(AccProfitFromPortfolio.portfolio_id == portfolio_id)
        
        if snapshot_at_gte is not None:
            query = query.filter(AccProfitFromPortfolio.snapshot_at >= snapshot_at_gte)
        
        if snapshot_at_lt is not None:
            query = query.filter(AccProfitFromPortfolio.snapshot_at < snapshot_at_lt)
        
        if snapshot_at_list:
            query = query.filter(AccProfitFromPortfolio.snapshot_at.in_(snapshot_at_list))
        
        total = query.count()
        items = query.order_by(desc(AccProfitFromPortfolio.snapshot_at)).offset(offset).limit(limit).all()
        
        return items, total

class ProfitAllocationRatioRepository(BaseRepository[ProfitAllocationRatio]):
    """收益分配比例Repository"""
    
    def __init__(self, db: Session):
        super().__init__(ProfitAllocationRatio, db)
    
    def create_allocation_ratio(
        self,
        portfolio_id: int,
        to_team_ratio: int,
        to_platform_ratio: int,
        to_user_ratio: int,
        created_by: int
    ) -> ProfitAllocationRatio:
        """创建收益分配比例"""
        # 获取当前最大版本号
        max_version = self.db.query(ProfitAllocationRatio.version).filter(
            ProfitAllocationRatio.portfolio_id == portfolio_id
        ).order_by(desc(ProfitAllocationRatio.version)).first()
        
        version = (max_version[0] if max_version else 0) + 1
        
        data = {
            "portfolio_id": portfolio_id,
            "version": version,
            "to_team_ratio": to_team_ratio,
            "to_platform_ratio": to_platform_ratio,
            "to_user_ratio": to_user_ratio,
            "created_by": created_by
        }
        return self.create(data)
    
    def get_latest_by_portfolio(self, portfolio_id: int) -> Optional[ProfitAllocationRatio]:
        """获取投资组合的最新分配比例"""
        return (
            self.db.query(ProfitAllocationRatio)
            .filter(ProfitAllocationRatio.portfolio_id == portfolio_id)
            .order_by(desc(ProfitAllocationRatio.version))
            .first()
        )
    
    def get_by_portfolio_ids(
        self,
        portfolio_ids: Optional[List[int]] = None,
        limit: int = 100,
        offset: int = 0
    ) -> tuple[List[ProfitAllocationRatio], int]:
        """按投资组合ID获取分配比例"""
        query = self.db.query(ProfitAllocationRatio)
        
        if portfolio_ids:
            query = query.filter(ProfitAllocationRatio.portfolio_id.in_(portfolio_ids))
        
        total = query.count()
        items = query.order_by(
            desc(ProfitAllocationRatio.version),
            desc(ProfitAllocationRatio.created_at)
        ).offset(offset).limit(limit).all()
        
        return items, total

class ProfitAllocationLogRepository(BaseRepository[ProfitAllocationLog]):
    """收益分配日志Repository"""
    
    def __init__(self, db: Session):
        super().__init__(ProfitAllocationLog, db)
    
    def create_allocation_log(
        self,
        portfolio_id: int,
        hour_end_at: int,
        hourly_snapshot_prev_id: int,
        hourly_snapshot_curr_id: int,
        hourly_profit: Decimal,
        profit_to_team: Decimal,
        profit_to_user: Decimal,
        profit_to_platform: Decimal,
        allocation_ratio_id: int
    ) -> ProfitAllocationLog:
        """创建收益分配日志"""
        data = {
            "portfolio_id": portfolio_id,
            "hour_end_at": hour_end_at,
            "hourly_snapshot_prev_id": hourly_snapshot_prev_id,
            "hourly_snapshot_curr_id": hourly_snapshot_curr_id,
            "hourly_profit": hourly_profit,
            "profit_to_team": profit_to_team,
            "profit_to_user": profit_to_user,
            "profit_to_platform": profit_to_platform,
            "allocation_ratio_id": allocation_ratio_id
        }
        return self.create(data)
    
    def get_by_time_range(
        self,
        portfolio_id: Optional[int] = None,
        hour_end_at_gte: Optional[int] = None,
        hour_end_at_lt: Optional[int] = None,
        hour_end_at_list: Optional[List[int]] = None,
        limit: int = 100,
        offset: int = 0
    ) -> tuple[List[ProfitAllocationLog], int]:
        """按时间范围获取分配日志"""
        query = self.db.query(ProfitAllocationLog)
        
        if portfolio_id is not None:
            query = query.filter(ProfitAllocationLog.portfolio_id == portfolio_id)
        
        if hour_end_at_gte is not None:
            query = query.filter(ProfitAllocationLog.hour_end_at >= hour_end_at_gte)
        
        if hour_end_at_lt is not None:
            query = query.filter(ProfitAllocationLog.hour_end_at < hour_end_at_lt)
        
        if hour_end_at_list:
            query = query.filter(ProfitAllocationLog.hour_end_at.in_(hour_end_at_list))
        
        total = query.count()
        items = query.order_by(desc(ProfitAllocationLog.hour_end_at)).offset(offset).limit(limit).all()
        
        return items, total

class ProfitWithdrawalRepository(BaseRepository[ProfitWithdrawal]):
    """提现记录Repository"""
    
    def __init__(self, db: Session):
        super().__init__(ProfitWithdrawal, db)
    
    def create_withdrawal(
        self,
        from_type: str,
        portfolio_id: Optional[int],
        chain_id: str,
        transaction_hash: str,
        transaction_time: int,
        usd_value: Decimal,
        assets: str,
        assets_amount: Decimal,
        created_by: int
    ) -> ProfitWithdrawal:
        """创建提现记录"""
        data = {
            "from_type": from_type,
            "portfolio_id": portfolio_id,
            "chain_id": chain_id,
            "transaction_hash": transaction_hash,
            "transaction_time": transaction_time,
            "usd_value": usd_value,
            "assets": assets,
            "assets_amount": assets_amount,
            "created_by": created_by
        }
        return self.create(data)
    
    def get_by_filters(
        self,
        from_type: Optional[str] = None,
        portfolio_id: Optional[int] = None,
        created_at_gte: Optional[int] = None,
        created_at_lt: Optional[int] = None,
        transaction_time_gte: Optional[int] = None,
        transaction_time_lt: Optional[int] = None,
        limit: int = 100,
        offset: int = 0
    ) -> tuple[List[ProfitWithdrawal], int]:
        """按过滤条件获取提现记录"""
        query = self.db.query(ProfitWithdrawal)
        
        if from_type:
            query = query.filter(ProfitWithdrawal.from_type == from_type)
        
        if portfolio_id is not None:
            query = query.filter(ProfitWithdrawal.portfolio_id == portfolio_id)
        
        if created_at_gte is not None:
            query = query.filter(ProfitWithdrawal.created_at >= created_at_gte)
        
        if created_at_lt is not None:
            query = query.filter(ProfitWithdrawal.created_at < created_at_lt)
        
        if transaction_time_gte is not None:
            query = query.filter(ProfitWithdrawal.transaction_time >= transaction_time_gte)
        
        if transaction_time_lt is not None:
            query = query.filter(ProfitWithdrawal.transaction_time < transaction_time_lt)
        
        total = query.count()
        items = query.order_by(desc(ProfitWithdrawal.created_at)).offset(offset).limit(limit).all()
        
        return items, total

class ProfitReallocationRepository(BaseRepository[ProfitReallocation]):
    """调账记录Repository"""
    
    def __init__(self, db: Session):
        super().__init__(ProfitReallocation, db)
    
    def create_reallocation(
        self,
        from_type: str,
        to_type: str,
        from_portfolio_id: Optional[int],
        to_portfolio_id: Optional[int],
        usd_value: Decimal,
        reason: str,
        created_by: int
    ) -> ProfitReallocation:
        """创建调账记录"""
        data = {
            "from_type": from_type,
            "to_type": to_type,
            "from_portfolio_id": from_portfolio_id,
            "to_portfolio_id": to_portfolio_id,
            "usd_value": usd_value,
            "reason": reason,
            "created_by": created_by
        }
        return self.create(data)

# 添加其他收益相关的Repository类...