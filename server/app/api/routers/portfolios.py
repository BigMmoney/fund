"""
Portfolio management router
"""
from typing import List, Optional
from fastapi import APIRouter, Depends, HTTPException, status, Query
from sqlalchemy.orm import Session
from sqlalchemy import or_

from server.app.database import get_db
from server.app.schemas import (
    PortfolioCreate, PortfolioUpdate, PortfolioResponse, 
    BaseResponse, PaginationParams, ListResponse, PortfolioTeamUpdate
)
from server.app.models import Portfolio, User, Team
from server.app.api.dependencies import get_current_active_user, require_portfolio_permission
from server.app.config import settings
from server.app.responses import StandardResponse

router = APIRouter(prefix="/portfolios", tags=["Portfolio Management"])


@router.get("", response_model=ListResponse)
async def list_portfolios(
    pagination: PaginationParams = Depends(),
    search: Optional[str] = Query(None, description="Search by portfolio name"),
    created_by: Optional[int] = Query(None, description="Filter by creator user ID"),
    db: Session = Depends(get_db),
    current_user: User = Depends(require_portfolio_permission)
):
    """List portfolios with optional filtering and pagination"""
    query = db.query(Portfolio)
    
    # Apply filters
    if search:
        query = query.filter(Portfolio.name.ilike(f"%{search}%"))
    
    if created_by:
        query = query.filter(Portfolio.created_by == created_by)
    
    # Get total count
    total = query.count()
    
    # Apply pagination
    offset = (pagination.page - 1) * pagination.page_size
    portfolios = query.offset(offset).limit(pagination.page_size).all()
    
    # Calculate pagination info
    total_pages = (total + pagination.page_size - 1) // pagination.page_size
    
    return {
        "data": portfolios,
        "pagination": {
            "total": total,
            "page": pagination.page,
            "page_size": pagination.page_size,
            "total_pages": total_pages
        }
    }


@router.get("/{portfolio_id}", response_model=PortfolioResponse)
async def get_portfolio(
    portfolio_id: int,
    db: Session = Depends(get_db),
    current_user: User = Depends(require_portfolio_permission)
):
    """Get portfolio by ID"""
    portfolio = db.query(Portfolio).filter(Portfolio.id == portfolio_id).first()
    if not portfolio:
        raise HTTPException(status_code=404, detail="Portfolio not found")
    
    return portfolio


@router.post("", response_model=PortfolioResponse)
async def create_portfolio(
    portfolio_data: PortfolioCreate,
    db: Session = Depends(get_db),
    current_user: User = Depends(require_portfolio_permission)
):
    """Create a new portfolio"""
    # Check if portfolio name already exists
    existing_portfolio = db.query(Portfolio).filter(
        Portfolio.name == portfolio_data.name
    ).first()
    if existing_portfolio:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Portfolio name already exists"
        )
    
    # Create new portfolio
    new_portfolio = Portfolio(
        name=portfolio_data.name,
        description=portfolio_data.description,
        allocation_percentage=portfolio_data.allocation_percentage,
        created_by=current_user.id
    )
    
    db.add(new_portfolio)
    db.commit()
    db.refresh(new_portfolio)
    
    return new_portfolio


@router.put("/{portfolio_id}", response_model=PortfolioResponse)
async def update_portfolio(
    portfolio_id: int,
    portfolio_data: PortfolioUpdate,
    db: Session = Depends(get_db),
    current_user: User = Depends(require_portfolio_permission)
):
    """Update portfolio"""
    portfolio = db.query(Portfolio).filter(Portfolio.id == portfolio_id).first()
    if not portfolio:
        raise HTTPException(status_code=404, detail="Portfolio not found")
    
    # Check if new name is already taken by another portfolio
    if portfolio_data.name and portfolio_data.name != portfolio.name:
        existing_portfolio = db.query(Portfolio).filter(
            Portfolio.name == portfolio_data.name,
            Portfolio.id != portfolio_id
        ).first()
        if existing_portfolio:
            raise HTTPException(
                status_code=status.HTTP_400_BAD_REQUEST,
                detail="Portfolio name already taken"
            )
    
    # Update fields
    update_data = portfolio_data.dict(exclude_unset=True)
    for field, value in update_data.items():
        setattr(portfolio, field, value)
    
    db.commit()
    db.refresh(portfolio)
    
    return portfolio


@router.delete("/{portfolio_id}", response_model=BaseResponse)
async def delete_portfolio(
    portfolio_id: int,
    db: Session = Depends(get_db),
    current_user: User = Depends(require_portfolio_permission)
):
    """Delete portfolio"""
    portfolio = db.query(Portfolio).filter(Portfolio.id == portfolio_id).first()
    if not portfolio:
        raise HTTPException(status_code=404, detail="Portfolio not found")
    
    # Check if portfolio has any related data (teams, profits, etc.)
    # This would need to be implemented based on business rules
    
    db.delete(portfolio)
    db.commit()
    
    return StandardResponse.success()


# API [18] PATCH /portfolios/{id}/team
@router.patch("/{portfolio_id}/team")
async def update_portfolio_team(
    portfolio_id: int,
    team_update: PortfolioTeamUpdate,
    db: Session = Depends(get_db),
    current_user: User = Depends(require_portfolio_permission)
):
    """
    API [18] - 更新portfolio与交易团队的绑定关系【权限验证 portfolio】
    """
    # 获取投资组合
    portfolio = db.query(Portfolio).filter(Portfolio.id == portfolio_id).first()
    if not portfolio:
        return StandardResponse.error("Portfolio not found")
    
    # 验证团队是否存在
    team = db.query(Team).filter(Team.id == team_update.teamId).first()
    if not team:
        return StandardResponse.error("Team not found")
    
    # 更新团队绑定
    portfolio.team_id = team_update.teamId
    db.commit()
    db.refresh(portfolio)
    
    # 返回完整的投资组合信息，按照需求文档格式
    return StandardResponse.object_success({
        "id": portfolio.id,
        "fundName": portfolio.fund_name,
        "fundAlias": portfolio.fund_alias,
        "inceptionTime": portfolio.inception_time,
        "accountName": portfolio.account_name,
        "accountAlias": portfolio.account_alias,
        "ceffuWalletId": portfolio.ceffu_wallet_id,
        "ceffuWalletName": portfolio.ceffu_wallet_name,
        "teamId": portfolio.team_id
    })


@router.get("/{portfolio_id}/wallet-mapping", response_model=dict)
async def get_portfolio_wallet_mapping(
    portfolio_id: int,
    db: Session = Depends(get_db),
    current_user: User = Depends(require_portfolio_permission)
):
    """Get Ceffu wallet mapping for portfolio"""
    portfolio = db.query(Portfolio).filter(Portfolio.id == portfolio_id).first()
    if not portfolio:
        raise HTTPException(status_code=404, detail="Portfolio not found")
    
    # Return the discovered wallet IDs from your previous work
    wallet_mapping = {
        "portfolio_id": portfolio_id,
        "portfolio_name": portfolio.name,
        "ceffu_wallets": {
            "zerodivision_btc": {
                "wallet_id": settings.zerodivision_btc_wallet_id,
                "display_name": "zerodivision-btc",
                "asset_type": "BTC"
            },
            "ci_usdt_zerod_bnb": {
                "wallet_id": settings.ci_usdt_zerod_bnb_wallet_id,
                "display_name": "CI-USDT-ZeroD-BNB",
                "asset_type": "USDT/BNB"
            }
        },
        "api_config": {
            "base_url": settings.ceffu_api_url,
            "api_key": settings.ceffu_api_key[:8] + "..." if settings.ceffu_api_key else "Not configured"
        }
    }
    
    return wallet_mapping


@router.get("/{portfolio_id}/balance", response_model=dict)
async def get_portfolio_balance(
    portfolio_id: int,
    db: Session = Depends(get_db),
    current_user: User = Depends(require_portfolio_permission)
):
    """Get portfolio balance from Ceffu API (placeholder for future implementation)"""
    portfolio = db.query(Portfolio).filter(Portfolio.id == portfolio_id).first()
    if not portfolio:
        raise HTTPException(status_code=404, detail="Portfolio not found")
    
    # This would integrate with your Ceffu API client
    # For now, return a placeholder structure
    balance_data = {
        "portfolio_id": portfolio_id,
        "portfolio_name": portfolio.name,
        "total_value_usd": 0.0,
        "wallets": [
            {
                "wallet_id": settings.zerodivision_btc_wallet_id,
                "name": "zerodivision-btc",
                "balance": {
                    "BTC": 0.0,
                    "USD_VALUE": 0.0
                }
            },
            {
                "wallet_id": settings.ci_usdt_zerod_bnb_wallet_id,
                "name": "CI-USDT-ZeroD-BNB", 
                "balance": {
                    "USDT": 0.0,
                    "BNB": 0.0,
                    "USD_VALUE": 0.0
                }
            }
        ],
        "last_updated": None,
        "note": "Balance fetching requires Ceffu API integration - placeholder data shown"
    }
    
    return balance_data