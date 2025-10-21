"""
系统服务
"""
from typing import Dict, Any
from sqlalchemy.orm import Session
from app.repositories.snapshots import NavSnapshotRepository, RateSnapshotRepository, AssetsSnapshotRepository
from app.repositories.profits import ProfitAllocationLogRepository
import logging

logger = logging.getLogger(__name__)

class SystemService:
    """系统服务"""
    
    def __init__(self, db: Session):
        self.db = db
        self.nav_snapshot_repo = NavSnapshotRepository(db)
        self.rate_snapshot_repo = RateSnapshotRepository(db)
        self.assets_snapshot_repo = AssetsSnapshotRepository(db)
        self.profit_log_repo = ProfitAllocationLogRepository(db)
    
    def get_system_status(self) -> Dict[str, Any]:
        """获取系统状态"""
        # 获取各种数据的最新处理时间戳
        watermark = self._calculate_watermark()
        
        return {
            "watermark": watermark
        }
    
    def _calculate_watermark(self) -> int:
        """计算系统数据水位线"""
        # 获取各种数据处理的最新时间戳
        timestamps = []
        
        # NAV快照最新时间
        nav_snapshots, _ = self.nav_snapshot_repo.get_multi(limit=1, order_by="snapshot_at", order_desc=True)
        if nav_snapshots:
            timestamps.append(nav_snapshots[0].snapshot_at)
        
        # 汇率快照最新时间
        rate_snapshots, _ = self.rate_snapshot_repo.get_multi(limit=1, order_by="snapshot_at", order_desc=True)
        if rate_snapshots:
            timestamps.append(rate_snapshots[0].snapshot_at)
        
        # 资产快照最新时间
        assets_snapshots, _ = self.assets_snapshot_repo.get_multi(limit=1, order_by="snapshot_at", order_desc=True)
        if assets_snapshots:
            timestamps.append(assets_snapshots[0].snapshot_at)
        
        # 收益分配日志最新时间
        profit_logs, _ = self.profit_log_repo.get_multi(limit=1, order_by="hour_end_at", order_desc=True)
        if profit_logs:
            timestamps.append(profit_logs[0].hour_end_at)
        
        # 返回最小的时间戳，表示所有数据都已处理到的时间点
        if timestamps:
            return min(timestamps)
        else:
            return 0