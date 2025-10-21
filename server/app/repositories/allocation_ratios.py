"""
分配比例数据访问层 (Repository)
"""
from typing import Optional, List
from sqlalchemy import select, desc
from sqlalchemy.ext.asyncio import AsyncSession
from server.app.models import ProfitAllocationRatio
from server.app.schemas import AllocationRatioCreate, AllocationRatioUpdate


class AllocationRatioRepository:
    """分配比例数据访问"""
    
    def __init__(self, db: AsyncSession):
        self.db = db
    
    async def get_by_id(self, ratio_id: int) -> Optional[ProfitAllocationRatio]:
        """
        根据 ID 获取分配比例
        
        Args:
            ratio_id: 分配比例 ID
            
        Returns:
            ProfitAllocationRatio 或 None
        """
        result = await self.db.execute(
            select(ProfitAllocationRatio).where(
                ProfitAllocationRatio.id == ratio_id
            )
        )
        return result.scalar_one_or_none()
    
    async def get_by_portfolio(
        self, 
        portfolio_id: int, 
        version: Optional[int] = None
    ) -> Optional[ProfitAllocationRatio]:
        """
        获取投资组合的分配比例
        
        Args:
            portfolio_id: 投资组合 ID
            version: 版本号 (None = 最新版本)
            
        Returns:
            ProfitAllocationRatio 或 None
        """
        query = select(ProfitAllocationRatio).where(
            ProfitAllocationRatio.portfolio_id == portfolio_id
        )
        
        if version is not None:
            query = query.where(ProfitAllocationRatio.version == version)
        else:
            # 获取最新版本
            query = query.order_by(desc(ProfitAllocationRatio.version)).limit(1)
        
        result = await self.db.execute(query)
        return result.scalar_one_or_none()
    
    async def list_by_portfolio(
        self, 
        portfolio_id: int,
        limit: int = 10
    ) -> List[ProfitAllocationRatio]:
        """
        获取投资组合的所有历史版本
        
        Args:
            portfolio_id: 投资组合 ID
            limit: 返回记录数限制
            
        Returns:
            分配比例列表 (按版本号降序)
        """
        result = await self.db.execute(
            select(ProfitAllocationRatio)
            .where(ProfitAllocationRatio.portfolio_id == portfolio_id)
            .order_by(desc(ProfitAllocationRatio.version))
            .limit(limit)
        )
        return list(result.scalars().all())
    
    async def create(
        self, 
        data: AllocationRatioCreate,
        created_by: int
    ) -> ProfitAllocationRatio:
        """
        创建分配比例
        
        自动递增版本号
        
        Args:
            data: 创建数据
            created_by: 创建人 ID
            
        Returns:
            新创建的 ProfitAllocationRatio
        """
        # 获取当前最大版本号
        current_max = await self.db.execute(
            select(ProfitAllocationRatio.version)
            .where(ProfitAllocationRatio.portfolio_id == data.portfolioId)
            .order_by(desc(ProfitAllocationRatio.version))
            .limit(1)
        )
        max_version = current_max.scalar_one_or_none() or 0
        
        # 创建新记录
        ratio = ProfitAllocationRatio(
            portfolio_id=data.portfolioId,
            version=max_version + 1,
            to_user=data.toUser,
            to_platform=data.toPlatform,
            to_team=data.toTeam,
            created_by=created_by
        )
        
        self.db.add(ratio)
        await self.db.commit()
        await self.db.refresh(ratio)
        
        return ratio
    
    async def update(
        self,
        ratio_id: int,
        data: AllocationRatioUpdate,
        updated_by: int
    ) -> Optional[ProfitAllocationRatio]:
        """
        更新分配比例
        
        注意: 这会创建新版本，而不是修改现有记录 (历史追踪)
        
        Args:
            ratio_id: 当前记录 ID
            data: 更新数据
            updated_by: 更新人 ID
            
        Returns:
            新版本的 ProfitAllocationRatio 或 None
        """
        # 获取当前记录
        current = await self.get_by_id(ratio_id)
        if not current:
            return None
        
        # 创建新版本
        new_ratio = ProfitAllocationRatio(
            portfolio_id=current.portfolio_id,
            version=current.version + 1,
            to_user=data.toUser if data.toUser is not None else current.to_user,
            to_platform=data.toPlatform if data.toPlatform is not None else current.to_platform,
            to_team=data.toTeam if data.toTeam is not None else current.to_team,
            created_by=updated_by
        )
        
        self.db.add(new_ratio)
        await self.db.commit()
        await self.db.refresh(new_ratio)
        
        return new_ratio
    
    async def delete(self, ratio_id: int) -> bool:
        """
        删除分配比例
        
        注意: 谨慎使用，通常不应删除历史记录
        
        Args:
            ratio_id: 分配比例 ID
            
        Returns:
            是否删除成功
        """
        ratio = await self.get_by_id(ratio_id)
        if not ratio:
            return False
        
        await self.db.delete(ratio)
        await self.db.commit()
        
        return True
    
    async def get_latest_version(self, portfolio_id: int) -> int:
        """
        获取投资组合的最新版本号
        
        Args:
            portfolio_id: 投资组合 ID
            
        Returns:
            最新版本号，如果不存在返回 0
        """
        result = await self.db.execute(
            select(ProfitAllocationRatio.version)
            .where(ProfitAllocationRatio.portfolio_id == portfolio_id)
            .order_by(desc(ProfitAllocationRatio.version))
            .limit(1)
        )
        return result.scalar_one_or_none() or 0
