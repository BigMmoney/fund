"""
快照数据Repository
"""
from typing import List, Optional, Dict, Any
from sqlalchemy.orm import Session
from sqlalchemy import and_
from app.models.snapshots import NavSnapshot, RateSnapshot, AssetsSnapshot
from server.app.base import BaseRepository

class NavSnapshotRepository(BaseRepository[NavSnapshot]):
    """NAV快照Repository"""
    
    def __init__(self, db: Session):
        super().__init__(NavSnapshot, db)
    
    def create_snapshot(self, snapshot_at: int, nav: float) -> NavSnapshot:
        """创建NAV快照"""
        snapshot_data = {
            "snapshot_at": snapshot_at,
            "nav": nav
        }
        return self.create(snapshot_data)
    
    def get_by_snapshot_time(self, snapshot_at: int) -> Optional[NavSnapshot]:
        """根据快照时间获取"""
        return self.db.query(NavSnapshot).filter(NavSnapshot.snapshot_at == snapshot_at).first()
    
    def get_snapshots_by_time_range(
        self,
        snapshot_at_gte: Optional[int] = None,
        snapshot_at_lt: Optional[int] = None,
        snapshot_at_list: Optional[List[int]] = None,
        limit: int = 100,
        offset: int = 0
    ) -> tuple[List[NavSnapshot], int]:
        """按时间范围获取快照"""
        filters = {}
        
        query = self.db.query(NavSnapshot)
        
        if snapshot_at_gte is not None:
            query = query.filter(NavSnapshot.snapshot_at >= snapshot_at_gte)
        
        if snapshot_at_lt is not None:
            query = query.filter(NavSnapshot.snapshot_at < snapshot_at_lt)
        
        if snapshot_at_list:
            query = query.filter(NavSnapshot.snapshot_at.in_(snapshot_at_list))
        
        total = query.count()
        items = query.order_by(NavSnapshot.snapshot_at.desc()).offset(offset).limit(limit).all()
        
        return items, total

class RateSnapshotRepository(BaseRepository[RateSnapshot]):
    """汇率快照Repository"""
    
    def __init__(self, db: Session):
        super().__init__(RateSnapshot, db)
    
    def create_snapshot(self, snapshot_at: int, exchange_rate: float) -> RateSnapshot:
        """创建汇率快照"""
        snapshot_data = {
            "snapshot_at": snapshot_at,
            "exchange_rate": exchange_rate
        }
        return self.create(snapshot_data)
    
    def get_snapshots_by_time_range(
        self,
        snapshot_at_gte: Optional[int] = None,
        snapshot_at_lt: Optional[int] = None,
        snapshot_at_list: Optional[List[int]] = None,
        limit: int = 100,
        offset: int = 0
    ) -> tuple[List[RateSnapshot], int]:
        """按时间范围获取快照"""
        query = self.db.query(RateSnapshot)
        
        if snapshot_at_gte is not None:
            query = query.filter(RateSnapshot.snapshot_at >= snapshot_at_gte)
        
        if snapshot_at_lt is not None:
            query = query.filter(RateSnapshot.snapshot_at < snapshot_at_lt)
        
        if snapshot_at_list:
            query = query.filter(RateSnapshot.snapshot_at.in_(snapshot_at_list))
        
        total = query.count()
        items = query.order_by(RateSnapshot.snapshot_at.desc()).offset(offset).limit(limit).all()
        
        return items, total

class AssetsSnapshotRepository(BaseRepository[AssetsSnapshot]):
    """资产快照Repository"""
    
    def __init__(self, db: Session):
        super().__init__(AssetsSnapshot, db)
    
    def create_snapshot(self, snapshot_at: int, assets_value: float) -> AssetsSnapshot:
        """创建资产快照"""
        snapshot_data = {
            "snapshot_at": snapshot_at,
            "assets_value": assets_value
        }
        return self.create(snapshot_data)
    
    def get_snapshots_by_time_range(
        self,
        snapshot_at_gte: Optional[int] = None,
        snapshot_at_lt: Optional[int] = None,
        snapshot_at_list: Optional[List[int]] = None,
        limit: int = 100,
        offset: int = 0
    ) -> tuple[List[AssetsSnapshot], int]:
        """按时间范围获取快照"""
        query = self.db.query(AssetsSnapshot)
        
        if snapshot_at_gte is not None:
            query = query.filter(AssetsSnapshot.snapshot_at >= snapshot_at_gte)
        
        if snapshot_at_lt is not None:
            query = query.filter(AssetsSnapshot.snapshot_at < snapshot_at_lt)
        
        if snapshot_at_list:
            query = query.filter(AssetsSnapshot.snapshot_at.in_(snapshot_at_list))
        
        total = query.count()
        items = query.order_by(AssetsSnapshot.snapshot_at.desc()).offset(offset).limit(limit).all()
        
        return items, total