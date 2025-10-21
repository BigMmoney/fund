"""
P1+ 数据采集管理API
Data Collection Management API

提供端点用于:
- 手动触发数据采集
- 查看采集器状态
- 查看采集统计
"""

from typing import Dict, Any, Optional
from datetime import datetime
from fastapi import APIRouter, Depends, Query
from sqlalchemy.orm import Session

from server.app.database import get_db
from server.app.auth import get_current_user
from server.app.models.user import User
from server.app.services.p1_scheduler import (
    p1_scheduler,
    trigger_manual_collection,
    get_scheduler_stats
)
from server.app.services.data_collector import collector_registry

router = APIRouter(prefix="/data-collection", tags=["Data Collection (P1+)"])


@router.get("/status")
async def get_collection_status(
    current_user: User = Depends(get_current_user)
) -> Dict[str, Any]:
    """
    获取数据采集系统状态
    
    返回:
    - 调度器运行状态
    - 注册的采集器列表
    - 采集统计信息
    """
    stats = get_scheduler_stats()
    
    # 获取所有注册的采集器
    collectors = [
        {
            'name': collector.name,
            'type': collector.__class__.__name__
        }
        for collector in collector_registry.get_all()
    ]
    
    return {
        'status': 'running' if stats['running'] else 'stopped',
        'collectors': collectors,
        'stats': stats
    }


@router.post("/trigger")
async def trigger_collection(
    timestamp: Optional[int] = Query(
        None,
        description="采集时间戳(秒)，不指定则使用当前整点时间"
    ),
    current_user: User = Depends(get_current_user),
    db: Session = Depends(get_db)
) -> Dict[str, Any]:
    """
    手动触发一次数据采集
    
    会立即运行所有注册的数据采集器
    
    参数:
    - timestamp: 可选，指定采集的时间戳
    
    需要权限: 登录用户
    """
    # 解析时间戳
    if timestamp:
        collection_time = datetime.fromtimestamp(timestamp)
    else:
        collection_time = datetime.now().replace(minute=0, second=0, microsecond=0)
    
    # 触发采集
    await trigger_manual_collection(collection_time)
    
    # 获取最新统计
    stats = get_scheduler_stats()
    
    return {
        'message': 'Data collection triggered',
        'collection_time': collection_time.strftime('%Y-%m-%d %H:%M'),
        'success': stats['last_run_success'],
        'stats': stats
    }


@router.get("/collectors")
async def list_collectors(
    current_user: User = Depends(get_current_user)
) -> Dict[str, Any]:
    """
    列出所有注册的数据采集器
    
    返回每个采集器的详细信息
    """
    collectors = []
    
    for collector in collector_registry.get_all():
        collectors.append({
            'name': collector.name,
            'type': collector.__class__.__name__,
            'description': collector.__class__.__doc__.strip() if collector.__class__.__doc__ else None
        })
    
    return {
        'total': len(collectors),
        'collectors': collectors
    }


@router.get("/history")
async def get_collection_history(
    limit: int = Query(100, ge=1, le=1000, description="返回记录数量"),
    offset: int = Query(0, ge=0, description="偏移量"),
    current_user: User = Depends(get_current_user),
    db: Session = Depends(get_db)
) -> Dict[str, Any]:
    """
    获取数据采集历史记录
    
    从数据库查询最近的采集记录
    
    TODO: 需要创建 collection_logs 表来记录每次采集的详细信息
    """
    # 临时实现：返回调度器统计
    stats = get_scheduler_stats()
    
    return {
        'message': 'Collection history endpoint (coming soon)',
        'current_stats': stats
    }


@router.post("/initialize")
async def initialize_collectors(
    current_user: User = Depends(get_current_user)
) -> Dict[str, Any]:
    """
    初始化数据采集器
    
    手动触发采集器初始化流程
    """
    try:
        if not p1_scheduler.collectors_initialized:
            await p1_scheduler.initialize_collectors()
            
            return {
                'message': 'Collectors initialized successfully',
                'collectors_count': len(collector_registry.get_all())
            }
        else:
            return {
                'message': 'Collectors already initialized',
                'collectors_count': len(collector_registry.get_all())
            }
            
    except Exception as e:
        return {
            'error': f'Failed to initialize collectors: {str(e)}'
        }
