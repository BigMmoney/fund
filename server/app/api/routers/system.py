"""
系统管理路由 - APIs [01], [12], [13], [14], [15]
系统状态、快照数据和权限管理
"""
from typing import Dict, Any, List, Optional
from fastapi import APIRouter, Depends, Query
from sqlalchemy.orm import Session
from datetime import datetime

from ...db.mysql import get_db
from server.app.models import SystemWatermark, Permission, NavSnapshot, RateSnapshot, AssetsSnapshot
from ...api.dependencies import get_current_user
from server.app.responses import StandardResponse

router = APIRouter(tags=["System Management"])


# API [01] GET /sys
@router.get("/sys")
async def get_system_status(
    current_user = Depends(get_current_user),
    db: Session = Depends(get_db)
) -> Dict[str, Any]:
    """系统状态数据 - 返回已经处理完毕的整点时间戳"""
    try:
        # 获取最新的系统水印
        watermark_record = db.query(SystemWatermark).order_by(
            SystemWatermark.updated_at.desc()
        ).first()
        
        # 如果没有记录，返回当前时间的前一个整点
        if not watermark_record:
            current_hour = datetime.utcnow().replace(minute=0, second=0, microsecond=0)
            watermark = int(current_hour.timestamp())
            
            # 创建初始记录
            new_record = SystemWatermark(watermark=watermark)
            db.add(new_record)
            db.commit()
        else:
            watermark = watermark_record.watermark
        
        return StandardResponse.object_success({
            "watermark": watermark
        })
        
    except Exception as e:
        return StandardResponse.error(f"Failed to get system status: {str(e)}")


# API [12] GET /nav_snapshots
@router.get("/nav_snapshots")
async def get_nav_snapshots(
    limit: int = Query(100, ge=1, le=1000),
    offset: int = Query(0, ge=0),
    snapshotAt_gte: Optional[int] = Query(None, description="整时快照秒级时间戳 >="),
    snapshotAt_lt: Optional[int] = Query(None, description="整时快照秒级时间戳 <"),
    snapshotAt: Optional[List[int]] = Query(None, description="多个条件时 snapshotAt=1&snapshotAt=2"),
    current_user = Depends(get_current_user),
    db: Session = Depends(get_db)
):
    """NAV的每小时快照，支持分页，默认时间倒序
    全部投资组合作为一个整体基金的NAV值"""
    try:
        query = db.query(NavSnapshot)
        
        # 应用过滤条件
        if snapshotAt_gte:
            query = query.filter(NavSnapshot.snapshot_at >= datetime.fromtimestamp(snapshotAt_gte))
        
        if snapshotAt_lt:
            query = query.filter(NavSnapshot.snapshot_at < datetime.fromtimestamp(snapshotAt_lt))
        
        if snapshotAt:
            timestamp_list = [datetime.fromtimestamp(ts) for ts in snapshotAt]
            query = query.filter(NavSnapshot.snapshot_at.in_(timestamp_list))
        
        # 默认时间倒序
        query = query.order_by(NavSnapshot.snapshot_at.desc())
        
        total = query.count()
        snapshots = query.offset(offset).limit(limit).all()
        
        snapshot_list = []
        for snapshot in snapshots:
            snapshot_list.append({
                "id": snapshot.id,
                "snapshotAt": int(snapshot.snapshot_at.timestamp()),
                "nav": float(snapshot.nav),
                "createdAt": int(snapshot.created_at.timestamp())
            })
        
        return StandardResponse.list_success(snapshot_list, total)
        
    except Exception as e:
        return StandardResponse.error(f"Failed to get NAV snapshots: {str(e)}")


# API [13] GET /rate_snapshots  
@router.get("/rate_snapshots")
async def get_rate_snapshots(
    limit: int = Query(100, ge=1, le=1000),
    offset: int = Query(0, ge=0),
    snapshotAt_gte: Optional[int] = Query(None, description="整时快照秒级时间戳 >="),
    snapshotAt_lt: Optional[int] = Query(None, description="整时快照秒级时间戳 <"),
    snapshotAt: Optional[List[int]] = Query(None, description="多个条件时 snapshotAt=1&snapshotAt=2"),
    current_user = Depends(get_current_user),
    db: Session = Depends(get_db)
):
    """ExchangeRate 每小时快照，支持分页，默认时间倒序排列"""
    try:
        query = db.query(RateSnapshot)
        
        # 应用过滤条件
        if snapshotAt_gte:
            query = query.filter(RateSnapshot.snapshot_at >= datetime.fromtimestamp(snapshotAt_gte))
        
        if snapshotAt_lt:
            query = query.filter(RateSnapshot.snapshot_at < datetime.fromtimestamp(snapshotAt_lt))
        
        if snapshotAt:
            timestamp_list = [datetime.fromtimestamp(ts) for ts in snapshotAt]
            query = query.filter(RateSnapshot.snapshot_at.in_(timestamp_list))
        
        # 默认时间倒序
        query = query.order_by(RateSnapshot.snapshot_at.desc())
        
        total = query.count()
        snapshots = query.offset(offset).limit(limit).all()
        
        snapshot_list = []
        for snapshot in snapshots:
            snapshot_list.append({
                "id": snapshot.id,
                "snapshotAt": int(snapshot.snapshot_at.timestamp()),
                "exchangeRate": float(snapshot.exchange_rate),
                "createdAt": int(snapshot.created_at.timestamp())
            })
        
        return StandardResponse.list_success(snapshot_list, total)
        
    except Exception as e:
        return StandardResponse.error(f"Failed to get rate snapshots: {str(e)}")


# API [14] GET /assets_snapshots
@router.get("/assets_snapshots")
async def get_assets_snapshots(
    limit: int = Query(100, ge=1, le=1000),
    offset: int = Query(0, ge=0),
    snapshotAt_gte: Optional[int] = Query(None, description="整点秒级时间戳 >="),
    snapshotAt_lt: Optional[int] = Query(None, description="整点秒级时间戳 <"),
    snapshotAt: Optional[List[int]] = Query(None, description="多个条件时 snapshotAt=1&snapshotAt=2"),
    current_user = Depends(get_current_user),
    db: Session = Depends(get_db)
):
    """净资产每小时快照，所有投资组合的净资产
    服务器需要加和每个整点时间每个投资组合的净资产"""
    try:
        query = db.query(AssetsSnapshot)
        
        # 应用过滤条件
        if snapshotAt_gte:
            query = query.filter(AssetsSnapshot.snapshot_at >= datetime.fromtimestamp(snapshotAt_gte))
        
        if snapshotAt_lt:
            query = query.filter(AssetsSnapshot.snapshot_at < datetime.fromtimestamp(snapshotAt_lt))
        
        if snapshotAt:
            timestamp_list = [datetime.fromtimestamp(ts) for ts in snapshotAt]
            query = query.filter(AssetsSnapshot.snapshot_at.in_(timestamp_list))
        
        # 默认时间倒序
        query = query.order_by(AssetsSnapshot.snapshot_at.desc())
        
        total = query.count()
        snapshots = query.offset(offset).limit(limit).all()
        
        snapshot_list = []
        for snapshot in snapshots:
            snapshot_list.append({
                "id": snapshot.id,
                "assetsValue": float(snapshot.assets_value),
                "snapshotAt": int(snapshot.snapshot_at.timestamp()),
                "createdAt": int(snapshot.created_at.timestamp())
            })
        
        return StandardResponse.list_success(snapshot_list, total)
        
    except Exception as e:
        return StandardResponse.error(f"Failed to get assets snapshots: {str(e)}")


# API [15] GET /permissions
@router.get("/permissions")
async def list_permissions(
    current_user = Depends(get_current_user),
    db: Session = Depends(get_db)
) -> Dict[str, Any]:
    """获取所有的系统权限，系统内置的，不需要更改"""
    try:
        permissions = db.query(Permission).all()
        
        permission_list = []
        for perm in permissions:
            permission_list.append({
                "id": perm.id,
                "label": perm.label,
                "description": perm.description or ""
            })
        
        return StandardResponse.list_success(permission_list, len(permission_list))
        
    except Exception as e:
        return StandardResponse.error(f"Failed to get permissions: {str(e)}")


# 辅助函数：更新系统水印
async def update_system_watermark(db: Session, new_watermark: int):
    """更新系统处理进度水印"""
    try:
        # 查找现有记录
        watermark_record = db.query(SystemWatermark).first()
        
        if watermark_record:
            watermark_record.watermark = new_watermark
            watermark_record.updated_at = datetime.utcnow()
        else:
            watermark_record = SystemWatermark(watermark=new_watermark)
            db.add(watermark_record)
        
        db.commit()
        return True
        
    except Exception as e:
        print(f"Failed to update watermark: {e}")
        return False


# 获取当前系统水印
def get_current_watermark(db: Session) -> int:
    """获取当前系统水印"""
    watermark_record = db.query(SystemWatermark).order_by(
        SystemWatermark.updated_at.desc()
    ).first()
    
    if watermark_record:
        return watermark_record.watermark
    
    # 如果没有记录，返回当前时间的前一个整点
    current_hour = datetime.utcnow().replace(minute=0, second=0, microsecond=0)
    return int(current_hour.timestamp())