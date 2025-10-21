"""
Data snapshots router for NAV, exchange rates, and asset tracking
"""
from typing import List, Optional, Dict, Any
from datetime import datetime, timedelta
from fastapi import APIRouter, Depends, HTTPException, status, Query, BackgroundTasks
from sqlalchemy.orm import Session
from sqlalchemy import desc, and_

from server.app.database import get_db
from server.app.schemas import (
    SnapshotCreate, SnapshotResponse, BaseResponse,
    PaginationParams, ListResponse, SnapshotType
)
from server.app.models import (
    NavSnapshot, RateSnapshot, AssetsSnapshot, 
    Portfolio, User
)
from server.app.api.dependencies import get_current_active_user, require_portfolio_permission
from server.app.config import settings
import logging

logger = logging.getLogger(__name__)

router = APIRouter(prefix="/snapshots", tags=["Data Snapshots"])


@router.get("/nav", response_model=ListResponse)
async def get_nav_snapshots(
    pagination: PaginationParams = Depends(),
    portfolio_id: Optional[int] = Query(None, description="Filter by portfolio ID"),
    snapshot_type: Optional[SnapshotType] = Query(None, description="Filter by snapshot type"),
    start_date: Optional[datetime] = Query(None, description="Start date filter"),
    end_date: Optional[datetime] = Query(None, description="End date filter"),
    db: Session = Depends(get_db),
    current_user: User = Depends(require_portfolio_permission)
):
    """Get NAV snapshots with filtering"""
    query = db.query(NavSnapshot)
    
    # Apply filters
    if portfolio_id:
        query = query.filter(NavSnapshot.portfolio_id == portfolio_id)
    
    if snapshot_type:
        query = query.filter(NavSnapshot.snapshot_type == snapshot_type.value)
    
    if start_date:
        query = query.filter(NavSnapshot.snapshot_date >= start_date)
    
    if end_date:
        query = query.filter(NavSnapshot.snapshot_date <= end_date)
    
    # Order by date descending
    query = query.order_by(desc(NavSnapshot.snapshot_date))
    
    # Get total count
    total = query.count()
    
    # Apply pagination
    offset = (pagination.page - 1) * pagination.page_size
    snapshots = query.offset(offset).limit(pagination.page_size).all()
    
    # Calculate pagination info
    total_pages = (total + pagination.page_size - 1) // pagination.page_size
    
    return {
        "data": snapshots,
        "pagination": {
            "total": total,
            "page": pagination.page,
            "page_size": pagination.page_size,
            "total_pages": total_pages
        }
    }


@router.get("/exchange-rates", response_model=ListResponse)
async def get_exchange_rate_snapshots(
    pagination: PaginationParams = Depends(),
    base_currency: Optional[str] = Query(None, description="Filter by base currency"),
    target_currency: Optional[str] = Query(None, description="Filter by target currency"),
    start_date: Optional[datetime] = Query(None, description="Start date filter"),
    end_date: Optional[datetime] = Query(None, description="End date filter"),
    db: Session = Depends(get_db),
    current_user: User = Depends(get_current_active_user)
):
    """Get exchange rate snapshots with filtering"""
    query = db.query(RateSnapshot)
    
    # Apply filters
    if base_currency:
        query = query.filter(RateSnapshot.base_currency == base_currency.upper())
    
    if target_currency:
        query = query.filter(RateSnapshot.target_currency == target_currency.upper())
    
    if start_date:
        query = query.filter(RateSnapshot.snapshot_date >= start_date)
    
    if end_date:
        query = query.filter(RateSnapshot.snapshot_date <= end_date)
    
    # Order by date descending
    query = query.order_by(desc(RateSnapshot.snapshot_date))
    
    # Get total count
    total = query.count()
    
    # Apply pagination
    offset = (pagination.page - 1) * pagination.page_size
    snapshots = query.offset(offset).limit(pagination.page_size).all()
    
    # Calculate pagination info
    total_pages = (total + pagination.page_size - 1) // pagination.page_size
    
    return {
        "data": snapshots,
        "pagination": {
            "total": total,
            "page": pagination.page,
            "page_size": pagination.page_size,
            "total_pages": total_pages
        }
    }


@router.get("/assets", response_model=ListResponse)
async def get_asset_snapshots(
    pagination: PaginationParams = Depends(),
    wallet_id: Optional[str] = Query(None, description="Filter by wallet ID"),
    asset_symbol: Optional[str] = Query(None, description="Filter by asset symbol"),
    start_date: Optional[datetime] = Query(None, description="Start date filter"),
    end_date: Optional[datetime] = Query(None, description="End date filter"),
    db: Session = Depends(get_db),
    current_user: User = Depends(require_portfolio_permission)
):
    """Get asset snapshots with filtering"""
    query = db.query(AssetsSnapshot)
    
    # Apply filters
    if wallet_id:
        query = query.filter(AssetsSnapshot.wallet_id == wallet_id)
    
    if asset_symbol:
        query = query.filter(AssetsSnapshot.asset_symbol == asset_symbol.upper())
    
    if start_date:
        query = query.filter(AssetsSnapshot.snapshot_date >= start_date)
    
    if end_date:
        query = query.filter(AssetsSnapshot.snapshot_date <= end_date)
    
    # Order by date descending
    query = query.order_by(desc(AssetsSnapshot.snapshot_date))
    
    # Get total count
    total = query.count()
    
    # Apply pagination
    offset = (pagination.page - 1) * pagination.page_size
    snapshots = query.offset(offset).limit(pagination.page_size).all()
    
    # Calculate pagination info
    total_pages = (total + pagination.page_size - 1) // pagination.page_size
    
    return {
        "data": snapshots,
        "pagination": {
            "total": total,
            "page": pagination.page,
            "page_size": pagination.page_size,
            "total_pages": total_pages
        }
    }


@router.post("/collect", response_model=BaseResponse)
async def trigger_snapshot_collection(
    background_tasks: BackgroundTasks,
    snapshot_type: SnapshotType = Query(SnapshotType.HOURLY, description="Type of snapshot to collect"),
    portfolio_id: Optional[int] = Query(None, description="Specific portfolio ID (optional)"),
    db: Session = Depends(get_db),
    current_user: User = Depends(require_portfolio_permission)
):
    """Trigger data snapshot collection manually"""
    
    # Add background task to collect snapshots
    background_tasks.add_task(
        collect_all_snapshots,
        db_session=db,
        snapshot_type=snapshot_type.value,
        portfolio_id=portfolio_id
    )
    
    return {
        "message": f"Snapshot collection triggered for {snapshot_type.value} data",
        "snapshot_type": snapshot_type.value,
        "portfolio_id": portfolio_id
    }


@router.get("/latest/nav/{portfolio_id}", response_model=Dict[str, Any])
async def get_latest_nav_snapshot(
    portfolio_id: int,
    db: Session = Depends(get_db),
    current_user: User = Depends(require_portfolio_permission)
):
    """Get latest NAV snapshot for a portfolio"""
    
    # Check if portfolio exists
    portfolio = db.query(Portfolio).filter(Portfolio.id == portfolio_id).first()
    if not portfolio:
        raise HTTPException(status_code=404, detail="Portfolio not found")
    
    # Get latest NAV snapshot
    latest_nav = db.query(NavSnapshot).filter(
        NavSnapshot.portfolio_id == portfolio_id
    ).order_by(desc(NavSnapshot.snapshot_date)).first()
    
    if not latest_nav:
        return {
            "portfolio_id": portfolio_id,
            "portfolio_name": portfolio.name,
            "latest_nav": None,
            "message": "No NAV snapshots available"
        }
    
    return {
        "portfolio_id": portfolio_id,
        "portfolio_name": portfolio.name,
        "latest_nav": {
            "nav_value": latest_nav.nav_value,
            "total_assets": latest_nav.total_assets,
            "total_liabilities": latest_nav.total_liabilities,
            "snapshot_date": latest_nav.snapshot_date,
            "snapshot_type": latest_nav.snapshot_type,
            "asset_breakdown": latest_nav.asset_breakdown
        }
    }


@router.get("/latest/exchange-rates", response_model=Dict[str, Any])
async def get_latest_exchange_rates(
    base_currency: str = Query("USD", description="Base currency"),
    db: Session = Depends(get_db),
    current_user: User = Depends(get_current_active_user)
):
    """Get latest exchange rates for a base currency"""
    
    # Get latest exchange rates
    latest_rates = db.query(RateSnapshot).filter(
        RateSnapshot.base_currency == base_currency.upper()
    ).order_by(desc(RateSnapshot.snapshot_date)).all()
    
    if not latest_rates:
        return {
            "base_currency": base_currency.upper(),
            "rates": {},
            "message": "No exchange rate data available"
        }
    
    # Group by target currency and get the latest rate for each
    rates_dict = {}
    latest_date = None
    
    for rate in latest_rates:
        if rate.target_currency not in rates_dict:
            rates_dict[rate.target_currency] = {
                "rate": rate.exchange_rate,
                "last_updated": rate.snapshot_date
            }
            if not latest_date or rate.snapshot_date > latest_date:
                latest_date = rate.snapshot_date
    
    return {
        "base_currency": base_currency.upper(),
        "last_updated": latest_date,
        "rates": rates_dict
    }


async def collect_all_snapshots(db_session: Session, snapshot_type: str, portfolio_id: Optional[int] = None):
    """Background task to collect all types of snapshots"""
    try:
        logger.info(f"Starting {snapshot_type} snapshot collection")
        
        # Collect NAV snapshots
        await collect_nav_snapshots(db_session, snapshot_type, portfolio_id)
        
        # Collect exchange rate snapshots
        await collect_exchange_rate_snapshots(db_session, snapshot_type)
        
        # Collect asset snapshots
        await collect_asset_snapshots(db_session, snapshot_type, portfolio_id)
        
        logger.info(f"Completed {snapshot_type} snapshot collection")
        
    except Exception as e:
        logger.error(f"Error in snapshot collection: {e}")


async def collect_nav_snapshots(db_session: Session, snapshot_type: str, portfolio_id: Optional[int] = None):
    """Collect NAV snapshots for portfolios"""
    try:
        query = db_session.query(Portfolio)
        if portfolio_id:
            query = query.filter(Portfolio.id == portfolio_id)
        
        portfolios = query.all()
        
        for portfolio in portfolios:
            # This would integrate with Ceffu API to get real data
            # For now, creating placeholder data
            nav_data = await calculate_portfolio_nav(portfolio)
            
            nav_snapshot = NavSnapshot(
                portfolio_id=portfolio.id,
                snapshot_type=snapshot_type,
                snapshot_date=datetime.utcnow(),
                nav_value=nav_data["nav_value"],
                total_assets=nav_data["total_assets"],
                total_liabilities=nav_data["total_liabilities"],
                asset_breakdown=nav_data["asset_breakdown"]
            )
            
            db_session.add(nav_snapshot)
        
        db_session.commit()
        logger.info(f"Created NAV snapshots for {len(portfolios)} portfolios")
        
    except Exception as e:
        logger.error(f"Error collecting NAV snapshots: {e}")
        db_session.rollback()


async def collect_exchange_rate_snapshots(db_session: Session, snapshot_type: str):
    """Collect exchange rate snapshots"""
    try:
        # Common currency pairs to track
        currency_pairs = [
            ("USD", "EUR"), ("USD", "GBP"), ("USD", "JPY"), 
            ("USD", "CNY"), ("BTC", "USD"), ("ETH", "USD"),
            ("BNB", "USD"), ("USDT", "USD")
        ]
        
        for base, target in currency_pairs:
            # This would integrate with real exchange rate API
            # For now, creating placeholder data
            exchange_rate = await get_exchange_rate(base, target)
            
            rate_snapshot = RateSnapshot(
                base_currency=base,
                target_currency=target,
                exchange_rate=exchange_rate,
                snapshot_date=datetime.utcnow(),
                snapshot_at=datetime.utcnow()
            )
            
            db_session.add(rate_snapshot)
        
        db_session.commit()
        logger.info(f"Created exchange rate snapshots for {len(currency_pairs)} pairs")
        
    except Exception as e:
        logger.error(f"Error collecting exchange rate snapshots: {e}")
        db_session.rollback()


async def collect_asset_snapshots(db_session: Session, snapshot_type: str, portfolio_id: Optional[int] = None):
    """Collect asset snapshots from Ceffu wallets"""
    try:
        # Use discovered wallet IDs
        wallet_ids = [
            settings.zerodivision_btc_wallet_id,
            settings.ci_usdt_zerod_bnb_wallet_id
        ]
        
        for wallet_id in wallet_ids:
            # This would integrate with Ceffu API to get real asset data
            # For now, creating placeholder data
            asset_data = await get_wallet_assets(wallet_id)
            
            for asset_symbol, balance in asset_data.items():
                asset_snapshot = AssetsSnapshot(
                    wallet_id=wallet_id,
                    asset_symbol=asset_symbol,
                    balance=balance["amount"],
                    assets_value=balance["usd_value"],
                    snapshot_date=datetime.utcnow(),
                    snapshot_at=datetime.utcnow()
                )
                
                db_session.add(asset_snapshot)
        
        db_session.commit()
        logger.info(f"Created asset snapshots for {len(wallet_ids)} wallets")
        
    except Exception as e:
        logger.error(f"Error collecting asset snapshots: {e}")
        db_session.rollback()


async def calculate_portfolio_nav(portfolio: Portfolio) -> Dict[str, Any]:
    """Calculate NAV for a portfolio (placeholder implementation)"""
    # This would integrate with Ceffu API for real calculation
    return {
        "nav_value": 100000.0,  # Placeholder
        "total_assets": 105000.0,
        "total_liabilities": 5000.0,
        "asset_breakdown": {
            "BTC": {"amount": 2.5, "usd_value": 70000.0},
            "ETH": {"amount": 15.0, "usd_value": 30000.0},
            "USDT": {"amount": 5000.0, "usd_value": 5000.0}
        }
    }


async def get_exchange_rate(base_currency: str, target_currency: str) -> float:
    """Get exchange rate (placeholder implementation)"""
    # This would integrate with real exchange rate API
    rate_map = {
        ("USD", "EUR"): 0.85,
        ("USD", "GBP"): 0.73,
        ("USD", "JPY"): 149.50,
        ("USD", "CNY"): 7.25,
        ("BTC", "USD"): 28000.0,
        ("ETH", "USD"): 2000.0,
        ("BNB", "USD"): 220.0,
        ("USDT", "USD"): 1.0
    }
    
    return rate_map.get((base_currency, target_currency), 1.0)


async def get_wallet_assets(wallet_id: str) -> Dict[str, Dict[str, Any]]:
    """Get assets from Ceffu wallet (placeholder implementation)"""
    # This would integrate with Ceffu API
    if wallet_id == settings.zerodivision_btc_wallet_id:
        return {
            "BTC": {
                "amount": 2.5,
                "usd_value": 70000.0,
                "metadata": {"wallet_name": "zerodivision-btc"}
            }
        }
    elif wallet_id == settings.ci_usdt_zerod_bnb_wallet_id:
        return {
            "USDT": {
                "amount": 5000.0,
                "usd_value": 5000.0,
                "metadata": {"wallet_name": "CI-USDT-ZeroD-BNB"}
            },
            "BNB": {
                "amount": 150.0,
                "usd_value": 33000.0,
                "metadata": {"wallet_name": "CI-USDT-ZeroD-BNB"}
            }
        }
    
    return {}