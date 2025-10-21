"""
快照数据模型
"""
from sqlalchemy import Column, Integer, String, Numeric, BigInteger
from .base import BaseModel

class NavSnapshot(BaseModel):
    """NAV快照模型"""
    __tablename__ = "nav_snapshots"
    
    snapshot_at = Column(BigInteger, nullable=False, index=True, comment="整时快照秒级时间?")
    nav = Column(Numeric(20, 8), nullable=False, comment="NAV?")

class RateSnapshot(BaseModel):
    """汇率快照模型"""
    __tablename__ = "rate_snapshots"
    
    snapshot_at = Column(BigInteger, nullable=False, index=True, comment="整时快照秒级时间戳")
    base_currency = Column(String(10), nullable=False, comment="基础货币")
    target_currency = Column(String(10), nullable=False, comment="目标货币")
    exchange_rate = Column(Numeric(20, 8), nullable=False, comment="汇率")
    source = Column(String(100), nullable=True, comment="数据来源")

class AssetsSnapshot(BaseModel):
    """资产快照模型"""
    __tablename__ = "assets_snapshots"
    
    snapshot_at = Column(BigInteger, nullable=True, index=True, comment="整时快照秒级时间戳")
    wallet_id = Column(Integer, nullable=True, index=True, comment="钱包ID")
    asset_symbol = Column(String(20), nullable=True, comment="资产符号")
    balance = Column(Numeric(30, 10), nullable=True, comment="余额")
    assets_value = Column(String(255), nullable=True, comment="资产USD价值")
