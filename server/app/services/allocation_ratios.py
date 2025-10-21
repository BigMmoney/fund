"""
分配比例业务逻辑层 (Service)
"""
from typing import Optional, List
from sqlalchemy.ext.asyncio import AsyncSession
from fastapi import HTTPException, status

from server.app.repositories.allocation_ratios import AllocationRatioRepository
from server.app.schemas import AllocationRatioCreate, AllocationRatioUpdate, AllocationRatioResponse
from server.app.models import ProfitAllocationRatio


class AllocationRatioService:
    """分配比例业务逻辑"""
    
    def __init__(self, db: AsyncSession):
        self.repo = AllocationRatioRepository(db)
        self.db = db
    
    async def get_by_id(self, ratio_id: int) -> Optional[AllocationRatioResponse]:
        """
        获取分配比例
        
        Args:
            ratio_id: 分配比例 ID
            
        Returns:
            AllocationRatioResponse 或 None
        """
        ratio = await self.repo.get_by_id(ratio_id)
        if not ratio:
            return None
        
        return self._to_response(ratio)
    
    async def get_latest_by_portfolio(
        self, 
        portfolio_id: int
    ) -> Optional[AllocationRatioResponse]:
        """
        获取投资组合的最新分配比例
        
        Args:
            portfolio_id: 投资组合 ID
            
        Returns:
            AllocationRatioResponse 或 None
        """
        ratio = await self.repo.get_by_portfolio(portfolio_id)
        if not ratio:
            return None
        
        return self._to_response(ratio)
    
    async def get_by_version(
        self,
        portfolio_id: int,
        version: int
    ) -> Optional[AllocationRatioResponse]:
        """
        获取投资组合的指定版本分配比例
        
        Args:
            portfolio_id: 投资组合 ID
            version: 版本号
            
        Returns:
            AllocationRatioResponse 或 None
        """
        ratio = await self.repo.get_by_portfolio(portfolio_id, version)
        if not ratio:
            return None
        
        return self._to_response(ratio)
    
    async def get_history(
        self, 
        portfolio_id: int,
        limit: int = 10
    ) -> List[AllocationRatioResponse]:
        """
        获取分配比例历史版本
        
        Args:
            portfolio_id: 投资组合 ID
            limit: 返回记录数限制
            
        Returns:
            分配比例响应列表
        """
        ratios = await self.repo.list_by_portfolio(portfolio_id, limit)
        return [self._to_response(r) for r in ratios]
    
    async def create(
        self, 
        data: AllocationRatioCreate,
        user_id: int
    ) -> AllocationRatioResponse:
        """
        创建分配比例
        
        Pydantic 已验证数据完整性:
        - toUser, toPlatform, toTeam 总和 = 100
        - 如果只提供 2 个值，第 3 个已自动计算
        
        Args:
            data: 创建数据
            user_id: 当前用户 ID
            
        Returns:
            AllocationRatioResponse
            
        Raises:
            HTTPException: 投资组合不存在或已有分配比例
        """
        # 验证投资组合是否存在
        if not await self._validate_portfolio_exists(data.portfolioId):
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=f"投资组合 {data.portfolioId} 不存在"
            )
        
        # 检查是否已有分配比例（可选，根据业务需求决定）
        # existing = await self.repo.get_by_portfolio(data.portfolioId)
        # if existing:
        #     raise HTTPException(
        #         status_code=status.HTTP_409_CONFLICT,
        #         detail=f"投资组合 {data.portfolioId} 已有分配比例配置"
        #     )
        
        # 创建分配比例
        ratio = await self.repo.create(data, user_id)
        
        return self._to_response(ratio)
    
    async def update(
        self,
        ratio_id: int,
        data: AllocationRatioUpdate,
        user_id: int
    ) -> Optional[AllocationRatioResponse]:
        """
        更新分配比例 (创建新版本)
        
        Pydantic 已验证数据完整性
        
        Args:
            ratio_id: 当前记录 ID
            data: 更新数据
            user_id: 当前用户 ID
            
        Returns:
            AllocationRatioResponse 或 None
        """
        ratio = await self.repo.update(ratio_id, data, user_id)
        if not ratio:
            return None
        
        return self._to_response(ratio)
    
    async def delete(self, ratio_id: int) -> bool:
        """
        删除分配比例
        
        Args:
            ratio_id: 分配比例 ID
            
        Returns:
            是否删除成功
        """
        return await self.repo.delete(ratio_id)
    
    async def _validate_portfolio_exists(self, portfolio_id: int) -> bool:
        """
        验证投资组合是否存在
        
        Args:
            portfolio_id: 投资组合 ID
            
        Returns:
            是否存在
        """
        # TODO: 调用 PortfolioRepository 验证
        # 目前先返回 True，后续集成时实现
        
        # from server.app.repositories.portfolios import PortfolioRepository
        # portfolio_repo = PortfolioRepository(self.db)
        # portfolio = await portfolio_repo.get_by_id(portfolio_id)
        # return portfolio is not None
        
        return True
    
    def _to_response(self, ratio: ProfitAllocationRatio) -> AllocationRatioResponse:
        """
        将 Model 转换为 Response
        
        Args:
            ratio: ProfitAllocationRatio 模型
            
        Returns:
            AllocationRatioResponse
        """
        return AllocationRatioResponse(
            id=ratio.id,
            portfolioId=ratio.portfolio_id,
            version=ratio.version,
            toUser=ratio.to_user,
            toPlatform=ratio.to_platform,
            toTeam=ratio.to_team,
            createdAt=ratio.created_at,
            createdBy=ratio.created_by,
            updatedAt=ratio.updated_at,
            updatedBy=ratio.updated_by
        )
