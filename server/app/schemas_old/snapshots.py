"""
快照数据相关模式
"""
from pydantic import BaseModel
from decimal import Decimal

class NavSnapshotResponse(BaseModel):
    """NAV快照响应模式"""
    id: int
    snapshot_at: int
    nav: Decimal
    created_at: int

    class Config:
        from_attributes = True

class RateSnapshotResponse(BaseModel):
    """汇率快照响应模式"""
    id: int
    snapshot_at: int
    exchange_rate: Decimal
    created_at: int

    class Config:
        from_attributes = True

class AssetsSnapshotResponse(BaseModel):
    """资产快照响应模式"""
    id: int
    snapshot_at: int
    assets_value: Decimal
    created_at: int

    class Config:
        from_attributes = True