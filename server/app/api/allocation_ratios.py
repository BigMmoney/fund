"""
分配比例 API 路由
"""
from typing import List
from fastapi import APIRouter, Depends, HTTPException, status
from sqlalchemy.ext.asyncio import AsyncSession

from server.app.database import get_db
from server.app.auth import get_current_user
from server.app.models import User
from server.app.services.allocation_ratios import AllocationRatioService
from server.app.schemas import (
    AllocationRatioCreate,
    AllocationRatioUpdate,
    AllocationRatioResponse
)

router = APIRouter(prefix="/allocation-ratios", tags=["分配比例管理"])


@router.post(
    "",
    response_model=AllocationRatioResponse,
    status_code=status.HTTP_201_CREATED,
    summary="创建分配比例",
    description="""
    创建投资组合的分配比例配置
    
    **输入方式**:
    1. 提供全部 3 个值: `{"portfolioId": 1, "toUser": 50, "toPlatform": 30, "toTeam": 20}`
    2. 提供 2 个值 (自动计算第 3 个):
       - `{"portfolioId": 1, "toUser": 50, "toPlatform": 30}` → toTeam = 20
       - `{"portfolioId": 1, "toUser": 50, "toTeam": 20}` → toPlatform = 30
       - `{"portfolioId": 1, "toPlatform": 30, "toTeam": 20}` → toUser = 50
    
    **验证规则**:
    - 每个值必须在 0-100 之间
    - 必须至少提供 2 个值
    - 总和必须等于 100
    
    **自动功能**:
    - 自动递增版本号
    - 自动记录创建人和创建时间
    """
)
async def create_allocation_ratio(
    data: AllocationRatioCreate,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user)
):
    """创建分配比例"""
    service = AllocationRatioService(db)
    
    try:
        ratio = await service.create(data, current_user.id)
        return ratio
    except HTTPException:
        raise
    except Exception as e:
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=f"创建分配比例失败: {str(e)}"
        )


@router.get(
    "/portfolio/{portfolio_id}",
    response_model=AllocationRatioResponse,
    summary="获取投资组合的最新分配比例",
    description="获取指定投资组合的最新版本分配比例配置"
)
async def get_latest_ratio(
    portfolio_id: int,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user)
):
    """获取投资组合的最新分配比例"""
    service = AllocationRatioService(db)
    
    ratio = await service.get_latest_by_portfolio(portfolio_id)
    if not ratio:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"投资组合 {portfolio_id} 没有配置分配比例"
        )
    
    return ratio


@router.get(
    "/portfolio/{portfolio_id}/version/{version}",
    response_model=AllocationRatioResponse,
    summary="获取投资组合的指定版本分配比例",
    description="获取指定投资组合的特定版本分配比例配置"
)
async def get_ratio_by_version(
    portfolio_id: int,
    version: int,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user)
):
    """获取投资组合的指定版本分配比例"""
    service = AllocationRatioService(db)
    
    ratio = await service.get_by_version(portfolio_id, version)
    if not ratio:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"投资组合 {portfolio_id} 的版本 {version} 不存在"
        )
    
    return ratio


@router.get(
    "/portfolio/{portfolio_id}/history",
    response_model=List[AllocationRatioResponse],
    summary="获取分配比例历史版本",
    description="""
    获取投资组合的分配比例历史版本列表
    
    **返回数据**:
    - 按版本号降序排列 (最新的在前)
    - 默认返回最近 10 个版本
    - 可通过 limit 参数调整返回数量
    """
)
async def get_ratio_history(
    portfolio_id: int,
    limit: int = 10,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user)
):
    """获取投资组合的分配比例历史版本"""
    if limit < 1 or limit > 100:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="limit 参数必须在 1-100 之间"
        )
    
    service = AllocationRatioService(db)
    ratios = await service.get_history(portfolio_id, limit)
    
    return ratios


@router.get(
    "/{ratio_id}",
    response_model=AllocationRatioResponse,
    summary="根据 ID 获取分配比例",
    description="根据分配比例 ID 获取详细信息"
)
async def get_ratio_by_id(
    ratio_id: int,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user)
):
    """根据 ID 获取分配比例"""
    service = AllocationRatioService(db)
    
    ratio = await service.get_by_id(ratio_id)
    if not ratio:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"分配比例 {ratio_id} 不存在"
        )
    
    return ratio


@router.put(
    "/{ratio_id}",
    response_model=AllocationRatioResponse,
    summary="更新分配比例",
    description="""
    更新分配比例 (创建新版本，保留历史记录)
    
    **注意事项**:
    - 更新操作会创建新版本，不会修改现有记录
    - 版本号自动递增
    - 保留完整的历史记录可追溯
    
    **验证规则**:
    与创建时相同的验证规则
    """
)
async def update_allocation_ratio(
    ratio_id: int,
    data: AllocationRatioUpdate,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user)
):
    """更新分配比例"""
    service = AllocationRatioService(db)
    
    try:
        ratio = await service.update(ratio_id, data, current_user.id)
        if not ratio:
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=f"分配比例 {ratio_id} 不存在"
            )
        
        return ratio
    except HTTPException:
        raise
    except Exception as e:
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=f"更新分配比例失败: {str(e)}"
        )


@router.delete(
    "/{ratio_id}",
    status_code=status.HTTP_204_NO_CONTENT,
    summary="删除分配比例",
    description="""
    删除指定的分配比例记录
    
    **警告**:
    - 此操作会永久删除记录
    - 通常不建议删除历史记录
    - 建议使用版本控制代替删除
    """
)
async def delete_allocation_ratio(
    ratio_id: int,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user)
):
    """删除分配比例"""
    service = AllocationRatioService(db)
    
    success = await service.delete(ratio_id)
    if not success:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"分配比例 {ratio_id} 不存在"
        )
    
    return None
