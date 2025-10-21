"""
Profit Management API Router - 严格按照API需求文档实现
APIs [20-33] 收益分配比例、提现、调账、快照等功能
"""
from fastapi import APIRouter, Depends, Query, HTTPException
from sqlalchemy.orm import Session
from sqlalchemy import desc, func
from typing import List, Optional
from enum import Enum
from datetime import datetime
import logging

from server.app.database import get_db
from server.app.models import (
    ProfitAllocationRatio, ProfitWithdrawal, ProfitReallocation,
    Portfolio, User
)
from server.app.api.dependencies import get_current_user, require_permission, require_profit_permission
from server.app.responses import StandardResponse, NotFoundError, ValidationError
from server.app.schemas import ListResponse, BaseResponse, PaginationParams

logger = logging.getLogger(__name__)

# 定义WithdrawalFromType枚举
class WithdrawalFromType(str, Enum):
    user = "user"
    team = "team"
    platform = "platform"

router = APIRouter(prefix="/profit", tags=["Profit Management"])


# API [20] GET /profit_allocation_ratios
@router.get("/profit_allocation_ratios")
async def get_profit_allocation_ratios(
    limit: int = Query(100, ge=1, le=1000),
    offset: int = Query(0, ge=0),
    portfolio_id: Optional[List[int]] = Query(None, description="portfolioId=1&portfolioId=2"),
    db: Session = Depends(get_db),
    current_user: User = Depends(get_current_user)
):
    """获取portfolio收益分配参数"""
    query = db.query(ProfitAllocationRatio)
    
    # 过滤条件
    if portfolio_id:
        query = query.filter(ProfitAllocationRatio.portfolio_id.in_(portfolio_id))
    
    # 根据version, createdAt倒序排列
    query = query.order_by(
        desc(ProfitAllocationRatio.version),
        desc(ProfitAllocationRatio.created_at)
    )
    
    # 分页
    total = query.count()
    ratios = query.offset(offset).limit(limit).all()
    
    # 格式化响应数据，使用API要求的字段名
    ratio_list = []
    for ratio in ratios:
        ratio_data = {
            "id": ratio.id,
            "portfolioId": ratio.portfolio_id,
            "version": ratio.version,
            "toTeamRatio": ratio.to_team,       # 数据库字段 to_team -> API字段 toTeamRatio
            "toPlatformRatio": ratio.to_platform,  # 数据库字段 to_platform -> API字段 toPlatformRatio
            "toUserRatio": ratio.to_user,       # 数据库字段 to_user -> API字段 toUserRatio
            "createdAt": int(ratio.created_at.timestamp()) if ratio.created_at else None,
            "createdBy": ratio.created_by
        }
        ratio_list.append(ratio_data)
    
    return StandardResponse.list_success(ratio_list, total)


# API [21] POST /profit_allocation_ratios
@router.post("/profit_allocation_ratios")
async def create_profit_allocation_ratio(
    request_data: dict,  # 接收完整的请求体
    db: Session = Depends(get_db),
    current_user: User = Depends(require_profit_permission)
):
    """添加新的portfolio分配比例"""
    # 获取请求参数（注意：portffolioId 是需求文档中的拼写错误，但我们需要严格遵循）
    portfolio_id_str = request_data.get("portffolioId")  # 注意拼写错误
    allocation = request_data.get("allocation", {})
    
    if not portfolio_id_str:
        return StandardResponse.error("portffolioId is required")
    
    try:
        portfolio_id = int(portfolio_id_str)
    except (ValueError, TypeError):
        return StandardResponse.error("portffolioId must be a valid number")
    
    # 验证分配比例总和
    to_team = allocation.get("toTeam", 0)
    to_platform = allocation.get("toPlatform", 0)
    to_user = allocation.get("toUser", 0)
    
    if to_team + to_platform + to_user != 10000:
        return StandardResponse.error("Total allocation ratio must equal 10000 (100%)")
    
    # 验证投资组合存在
    portfolio = db.query(Portfolio).filter(Portfolio.id == portfolio_id).first()
    if not portfolio:
        return StandardResponse.error("Portfolio not found")
    
    try:
        # 获取当前最新版本号
        latest_version = db.query(func.max(ProfitAllocationRatio.version)).filter(
            ProfitAllocationRatio.portfolio_id == portfolio_id
        ).scalar() or 0
        
        # 创建新的分配比例记录
        new_ratio = ProfitAllocationRatio(
            portfolio_id=portfolio_id,
            version=latest_version + 1,
            to_team=to_team,
            to_platform=to_platform,
            to_user=to_user,
            created_by=current_user.id
        )
        
        db.add(new_ratio)
        db.commit()
        db.refresh(new_ratio)
        
        # 格式化响应数据
        ratio_data = {
            "id": new_ratio.id,
            "portfolioId": new_ratio.portfolio_id,
            "version": new_ratio.version,
            "toTeam": new_ratio.to_team,      
            "toPlatform": new_ratio.to_platform,
            "toUser": new_ratio.to_user,
            "createdAt": int(new_ratio.created_at.timestamp()) if new_ratio.created_at else None
        }
        
        return StandardResponse.object_success(ratio_data)
        
    except Exception as e:
        return StandardResponse.error(f"Failed to create profit allocation ratio: {str(e)}")


# API [22] GET /profit_allocation_ratios/{id}
@router.get("/profit_allocation_ratios/{id}")
async def get_profit_allocation_ratio(
    id: int,
    db: Session = Depends(get_db),
    current_user: User = Depends(get_current_user)
):
    """获取某个收益分配比例"""
    ratio = db.query(ProfitAllocationRatio).filter(ProfitAllocationRatio.id == id).first()
    if not ratio:
        raise NotFoundError("Profit allocation ratio not found")
    
    ratio_data = {
        "id": ratio.id,
        "portfolioId": ratio.portfolio_id,
        "version": ratio.version,
        "toTeam": ratio.to_team,
        "toPlatform": ratio.to_platform,
        "toUser": ratio.to_user,
        "createdAt": int(ratio.created_at.timestamp()) if ratio.created_at else None
    }
    
    return StandardResponse.object_success(ratio_data)


# ==================== 收益提取管理 ====================

@router.get("/profit_withdrawals", response_model=ListResponse)
async def get_withdrawals(
    pagination: PaginationParams = Depends(),
    from_type: Optional[WithdrawalFromType] = Query(None, description="Filter by withdrawal source type"),
    team_id: Optional[int] = Query(None, description="Filter by team ID"),
    start_date: Optional[datetime] = Query(None, description="Start date filter"),
    end_date: Optional[datetime] = Query(None, description="End date filter"),
    db: Session = Depends(get_db),
    current_user: User = Depends(require_profit_permission)
):
    """获取收益提取记录"""
    query = db.query(ProfitWithdrawal)
    
    # 过滤条件
    if from_type:
        query = query.filter(ProfitWithdrawal.from_type == from_type)
    
    if team_id:
        query = query.filter(ProfitWithdrawal.team_id == team_id)
    
    if start_date:
        query = query.filter(ProfitWithdrawal.transaction_time >= start_date)
    
    if end_date:
        query = query.filter(ProfitWithdrawal.transaction_time <= end_date)
    
    # 按交易时间降序排序
    query = query.order_by(desc(ProfitWithdrawal.transaction_time))
    
    # 分页
    total = query.count()
    withdrawals = query.offset(pagination.skip).limit(pagination.limit).all()
    
    return ListResponse(
        data=[
            ProfitWithdrawalResponse(
                id=withdrawal.id,
                from_type=withdrawal.from_type,
                team_id=withdrawal.team_id,
                chain_id=withdrawal.chain_id,
                transaction_hash=withdrawal.transaction_hash,
                transaction_time=withdrawal.transaction_time,
                usd_value=float(withdrawal.usd_value),
                assets=withdrawal.assets,
                assets_amount=float(withdrawal.assets_amount),
                created_at=withdrawal.created_at,
                created_by=withdrawal.created_by
            ) for withdrawal in withdrawals
        ],
        pagination={
            "total": total,
            "page": pagination.page,
            "size": pagination.size,
            "pages": (total + pagination.size - 1) // pagination.size
        }
    )


@router.post("/profit_withdrawals", response_model=BaseResponse)
async def create_withdrawal(
    withdrawal_data: ProfitWithdrawalCreate,
    db: Session = Depends(get_db),
    current_user: User = Depends(require_profit_permission)
):
    """记录收益提取操作"""
    try:
        # 验证交易哈希是否已存在
        existing = db.query(ProfitWithdrawal).filter(
            ProfitWithdrawal.transaction_hash == withdrawal_data.transaction_hash
        ).first()
        if existing:
            raise HTTPException(
                status_code=400,
                detail="Transaction hash already exists"
            )
        
        # 如果是团队提取，验证团队是否存在
        if withdrawal_data.from_type == WithdrawalFromType.team:
            if not withdrawal_data.team_id:
                raise HTTPException(
                    status_code=400,
                    detail="Team ID is required for team withdrawals"
                )
            
            team = db.query(Team).filter(Team.id == withdrawal_data.team_id).first()
            if not team:
                raise HTTPException(status_code=404, detail="Team not found")
        
        # 创建提取记录
        new_withdrawal = ProfitWithdrawal(
            from_type=withdrawal_data.from_type,
            team_id=withdrawal_data.team_id,
            chain_id=withdrawal_data.chain_id,
            transaction_hash=withdrawal_data.transaction_hash,
            transaction_time=withdrawal_data.transaction_time,
            usd_value=withdrawal_data.usd_value,
            assets=withdrawal_data.assets,
            assets_amount=withdrawal_data.assets_amount,
            created_by=current_user.id
        )
        
        db.add(new_withdrawal)
        db.commit()
        db.refresh(new_withdrawal)
        
        logger.info(f"Created withdrawal record: {withdrawal_data.transaction_hash}")
        
        return BaseResponse(
            success=True,
            message="Withdrawal record created successfully",
            data={"id": new_withdrawal.id}
        )
        
    except HTTPException:
        raise
    except Exception as e:
        logger.error(f"Error creating withdrawal record: {e}")
        db.rollback()
        raise HTTPException(status_code=500, detail="Internal server error")


# ==================== 收益调账管理 ====================

@router.get("/profit_reallocations", response_model=ListResponse)
async def get_reallocations(
    pagination: PaginationParams = Depends(),
    from_type: Optional[ReallocationFromType] = Query(None, description="Filter by source type"),
    to_type: Optional[ReallocationToType] = Query(None, description="Filter by target type"),
    start_date: Optional[datetime] = Query(None, description="Start date filter"),
    end_date: Optional[datetime] = Query(None, description="End date filter"),
    db: Session = Depends(get_db),
    current_user: User = Depends(require_profit_permission)
):
    """获取收益调账记录"""
    query = db.query(ProfitReallocation)
    
    # 过滤条件
    if from_type:
        query = query.filter(ProfitReallocation.from_type == from_type)
    
    if to_type:
        query = query.filter(ProfitReallocation.to_type == to_type)
    
    if start_date:
        query = query.filter(ProfitReallocation.created_at >= start_date)
    
    if end_date:
        query = query.filter(ProfitReallocation.created_at <= end_date)
    
    # 按创建时间降序排序
    query = query.order_by(desc(ProfitReallocation.created_at))
    
    # 分页
    total = query.count()
    reallocations = query.offset(pagination.skip).limit(pagination.limit).all()
    
    return ListResponse(
        data=[
            ProfitReallocationResponse(
                id=reallocation.id,
                from_type=reallocation.from_type,
                to_type=reallocation.to_type,
                from_team_id=reallocation.from_team_id,
                to_team_id=reallocation.to_team_id,
                usd_value=float(reallocation.usd_value),
                reason=reallocation.reason,
                created_at=reallocation.created_at,
                created_by=reallocation.created_by
            ) for reallocation in reallocations
        ],
        pagination={
            "total": total,
            "page": pagination.page,
            "size": pagination.size,
            "pages": (total + pagination.size - 1) // pagination.size
        }
    )


@router.post("/profit_reallocations", response_model=BaseResponse)
async def create_reallocation(
    reallocation_data: ProfitReallocationCreate,
    db: Session = Depends(get_db),
    current_user: User = Depends(require_profit_permission)
):
    """创建收益调账记录"""
    try:
        # 验证团队ID（如果涉及团队）
        if reallocation_data.from_type == ReallocationFromType.team:
            if not reallocation_data.from_team_id:
                raise HTTPException(
                    status_code=400,
                    detail="From team ID is required for team reallocations"
                )
            
            team = db.query(Team).filter(Team.id == reallocation_data.from_team_id).first()
            if not team:
                raise HTTPException(status_code=404, detail="From team not found")
        
        if reallocation_data.to_type == ReallocationToType.team:
            if not reallocation_data.to_team_id:
                raise HTTPException(
                    status_code=400,
                    detail="To team ID is required for team reallocations"
                )
            
            team = db.query(Team).filter(Team.id == reallocation_data.to_team_id).first()
            if not team:
                raise HTTPException(status_code=404, detail="To team not found")
        
        # 创建调账记录
        new_reallocation = ProfitReallocation(
            from_type=reallocation_data.from_type,
            to_type=reallocation_data.to_type,
            from_team_id=reallocation_data.from_team_id,
            to_team_id=reallocation_data.to_team_id,
            usd_value=reallocation_data.usd_value,
            reason=reallocation_data.reason,
            created_by=current_user.id
        )
        
        db.add(new_reallocation)
        db.commit()
        db.refresh(new_reallocation)
        
        logger.info(f"Created reallocation record: {reallocation_data.from_type} -> {reallocation_data.to_type}")
        
        return BaseResponse(
            success=True,
            message="Reallocation record created successfully",
            data={"id": new_reallocation.id}
        )
        
    except HTTPException:
        raise
    except Exception as e:
        logger.error(f"Error creating reallocation record: {e}")
        db.rollback()
        raise HTTPException(status_code=500, detail="Internal server error")


# ==================== 收益汇总统计 ====================

@router.get("/summary", response_model=BaseResponse)
async def get_profit_summary(
    team_id: Optional[int] = Query(None, description="Filter by team ID"),
    start_date: Optional[datetime] = Query(None, description="Start date filter"),
    end_date: Optional[datetime] = Query(None, description="End date filter"),
    db: Session = Depends(get_db),
    current_user: User = Depends(require_profit_permission)
):
    """获取收益汇总统计"""
    try:
        # 默认时间范围：最近30天
        if not end_date:
            end_date = datetime.utcnow()
        if not start_date:
            start_date = end_date - timedelta(days=30)
        
        # 获取最新的累计收益快照
        latest_user_profit = db.query(AccProfitUserSnapshot).filter(
            AccProfitUserSnapshot.snapshot_at >= start_date,
            AccProfitUserSnapshot.snapshot_at <= end_date
        ).order_by(desc(AccProfitUserSnapshot.snapshot_at)).first()
        
        latest_platform_profit = db.query(AccProfitPlatformSnapshot).filter(
            AccProfitPlatformSnapshot.snapshot_at >= start_date,
            AccProfitPlatformSnapshot.snapshot_at <= end_date
        ).order_by(desc(AccProfitPlatformSnapshot.snapshot_at)).first()
        
        # 团队收益
        team_profit_query = db.query(AccProfitTeamSnapshot).filter(
            AccProfitTeamSnapshot.snapshot_at >= start_date,
            AccProfitTeamSnapshot.snapshot_at <= end_date
        )
        
        if team_id:
            # 查找该团队相关的投资组合
            portfolios = db.query(Portfolio).filter(Portfolio.team_id == team_id).all()
            portfolio_ids = [p.id for p in portfolios]
            if portfolio_ids:
                team_profit_query = team_profit_query.filter(
                    AccProfitTeamSnapshot.portfolio_id.in_(portfolio_ids)
                )
        
        team_profits = team_profit_query.order_by(desc(AccProfitTeamSnapshot.snapshot_at)).limit(50).all()
        
        # 收益提取统计
        withdrawal_query = db.query(ProfitWithdrawal).filter(
            ProfitWithdrawal.transaction_time >= start_date,
            ProfitWithdrawal.transaction_time <= end_date
        )
        
        if team_id:
            withdrawal_query = withdrawal_query.filter(ProfitWithdrawal.team_id == team_id)
        
        # 按类型统计提取金额
        team_withdrawals = withdrawal_query.filter(
            ProfitWithdrawal.from_type == WithdrawalFromType.team
        ).all()
        
        platform_withdrawals = withdrawal_query.filter(
            ProfitWithdrawal.from_type == WithdrawalFromType.platform
        ).all()
        
        # 调账统计
        reallocation_query = db.query(ProfitReallocation).filter(
            ProfitReallocation.created_at >= start_date,
            ProfitReallocation.created_at <= end_date
        )
        
        reallocations = reallocation_query.all()
        
        # 构建响应数据
        summary = {
            "period": {
                "start_date": start_date.isoformat(),
                "end_date": end_date.isoformat()
            },
            "accumulated_profits": {
                "user": float(latest_user_profit.acc_profit) if latest_user_profit else 0.0,
                "platform": float(latest_platform_profit.acc_profit) if latest_platform_profit else 0.0,
                "team": sum(float(tp.acc_profit) for tp in team_profits)
            },
            "withdrawals": {
                "team": {
                    "count": len(team_withdrawals),
                    "total_usd": sum(float(w.usd_value) for w in team_withdrawals)
                },
                "platform": {
                    "count": len(platform_withdrawals),
                    "total_usd": sum(float(w.usd_value) for w in platform_withdrawals)
                }
            },
            "reallocations": {
                "count": len(reallocations),
                "total_usd": sum(float(r.usd_value) for r in reallocations)
            }
        }
        
        return BaseResponse(
            success=True,
            message="Profit summary retrieved successfully",
            data=summary
        )
        
    except Exception as e:
        logger.error(f"Error getting profit summary: {e}")
        raise HTTPException(status_code=500, detail="Internal server error")


# ==================== 收益分配计算 ====================

@router.post("/calculate-allocation", response_model=BaseResponse)
async def calculate_profit_allocation(
    portfolio_id: int,
    profit_amount: float,
    db: Session = Depends(get_db),
    current_user: User = Depends(require_profit_permission)
):
    """计算收益分配"""
    try:
        # 获取投资组合的最新分配比例
        latest_ratio = db.query(ProfitAllocationRatio).filter(
            ProfitAllocationRatio.portfolio_id == portfolio_id
        ).order_by(desc(ProfitAllocationRatio.version)).first()
        
        if not latest_ratio:
            raise HTTPException(
                status_code=404,
                detail="No allocation ratio found for this portfolio"
            )
        
        # 计算分配金额
        total_ratio = latest_ratio.to_team + latest_ratio.to_platform + latest_ratio.to_user
        
        allocation = {
            "portfolio_id": portfolio_id,
            "profit_amount": profit_amount,
            "allocation_ratio": {
                "team": latest_ratio.to_team,
                "platform": latest_ratio.to_platform,
                "user": latest_ratio.to_user,
                "version": latest_ratio.version
            },
            "allocation_amounts": {
                "team": profit_amount * (latest_ratio.to_team / total_ratio),
                "platform": profit_amount * (latest_ratio.to_platform / total_ratio),
                "user": profit_amount * (latest_ratio.to_user / total_ratio)
            }
        }
        
        return BaseResponse(
            success=True,
            message="Profit allocation calculated successfully",
            data=allocation
        )
        
    except HTTPException:
        raise
    except Exception as e:
        logger.error(f"Error calculating profit allocation: {e}")
        raise HTTPException(status_code=500, detail="Internal server error")