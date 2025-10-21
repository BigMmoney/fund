"""
Ceffu API integration router
"""
from typing import Dict, Any
from fastapi import APIRouter, Depends, HTTPException, BackgroundTasks
from sqlalchemy.orm import Session

from server.app.database import get_db
from server.app.schemas import BaseResponse
from server.app.models import User
from server.app.api.dependencies import get_current_active_user, require_portfolio_permission
from server.app.services.ceffu_client import test_ceffu_integration, get_portfolio_data, ceffu_client
from server.app.config import settings
import logging

logger = logging.getLogger(__name__)

router = APIRouter(prefix="/ceffu", tags=["Ceffu Integration"])


@router.get("/test", response_model=Dict[str, Any])
async def test_ceffu_connection(
    current_user: User = Depends(get_current_active_user)
):
    """Test Ceffu API connection and wallet access"""
    try:
        results = await test_ceffu_integration()
        return {
            "success": True,
            "message": "Ceffu integration test completed",
            "results": results,
            "configured_wallets": {
                "zerodivision_btc": settings.zerodivision_btc_wallet_id,
                "ci_usdt_zerod_bnb": settings.ci_usdt_zerod_bnb_wallet_id
            }
        }
    except Exception as e:
        logger.error(f"Ceffu integration test failed: {e}")
        raise HTTPException(status_code=500, detail=f"Integration test failed: {str(e)}")


@router.get("/wallets/balance", response_model=Dict[str, Any])
async def get_all_wallet_balances(
    current_user: User = Depends(require_portfolio_permission)
):
    """Get balance for all configured wallets"""
    try:
        balances = await ceffu_client.get_all_wallets_balance()
        
        # Calculate summary
        total_value = sum(
            balance.get("total_value_usd", 0) 
            for balance in balances.values()
        )
        
        return {
            "success": True,
            "total_value_usd": total_value,
            "wallet_count": len(balances),
            "wallets": balances,
            "last_updated": balances.get(list(balances.keys())[0], {}).get("timestamp") if balances else None
        }
        
    except Exception as e:
        logger.error(f"Error getting wallet balances: {e}")
        raise HTTPException(status_code=500, detail=f"Failed to get wallet balances: {str(e)}")


@router.get("/wallets/{wallet_id}/balance", response_model=Dict[str, Any])
async def get_wallet_balance(
    wallet_id: str,
    current_user: User = Depends(require_portfolio_permission)
):
    """Get balance for a specific wallet"""
    try:
        # Validate wallet ID
        valid_wallets = [
            settings.zerodivision_btc_wallet_id,
            settings.ci_usdt_zerod_bnb_wallet_id
        ]
        
        if wallet_id not in valid_wallets:
            raise HTTPException(status_code=404, detail="Wallet not found or not configured")
        
        balance = await ceffu_client.get_wallet_balance(wallet_id)
        
        return {
            "success": True,
            "wallet": balance
        }
        
    except HTTPException:
        raise
    except Exception as e:
        logger.error(f"Error getting wallet balance for {wallet_id}: {e}")
        raise HTTPException(status_code=500, detail=f"Failed to get wallet balance: {str(e)}")


@router.get("/wallets/{wallet_id}/transactions", response_model=Dict[str, Any])
async def get_wallet_transactions(
    wallet_id: str,
    limit: int = 100,
    current_user: User = Depends(require_portfolio_permission)
):
    """Get transactions for a specific wallet"""
    try:
        # Validate wallet ID
        valid_wallets = [
            settings.zerodivision_btc_wallet_id,
            settings.ci_usdt_zerod_bnb_wallet_id
        ]
        
        if wallet_id not in valid_wallets:
            raise HTTPException(status_code=404, detail="Wallet not found or not configured")
        
        transactions = await ceffu_client.get_wallet_transactions(wallet_id, limit)
        
        return {
            "success": True,
            "wallet_id": wallet_id,
            "transaction_count": len(transactions),
            "transactions": transactions
        }
        
    except HTTPException:
        raise
    except Exception as e:
        logger.error(f"Error getting wallet transactions for {wallet_id}: {e}")
        raise HTTPException(status_code=500, detail=f"Failed to get wallet transactions: {str(e)}")


@router.get("/portfolio/{portfolio_id}/data", response_model=Dict[str, Any])
async def get_ceffu_portfolio_data(
    portfolio_id: int,
    db: Session = Depends(get_db),
    current_user: User = Depends(require_portfolio_permission)
):
    """Get comprehensive portfolio data from Ceffu API"""
    try:
        # Verify portfolio exists
        from app.models import Portfolio
        portfolio = db.query(Portfolio).filter(Portfolio.id == portfolio_id).first()
        if not portfolio:
            raise HTTPException(status_code=404, detail="Portfolio not found")
        
        portfolio_data = await get_portfolio_data(portfolio_id)
        
        return {
            "success": True,
            "portfolio_name": portfolio.name,
            "data": portfolio_data
        }
        
    except HTTPException:
        raise
    except Exception as e:
        logger.error(f"Error getting portfolio data for {portfolio_id}: {e}")
        raise HTTPException(status_code=500, detail=f"Failed to get portfolio data: {str(e)}")


@router.get("/config", response_model=Dict[str, Any])
async def get_ceffu_config(
    current_user: User = Depends(get_current_active_user)
):
    """Get Ceffu API configuration (without sensitive data)"""
    return {
        "api_url": settings.ceffu_api_url,
        "api_key_configured": bool(settings.ceffu_api_key),
        "secret_key_configured": bool(settings.ceffu_secret_key),
        "configured_wallets": {
            "zerodivision_btc": {
                "wallet_id": settings.zerodivision_btc_wallet_id,
                "display_name": "zerodivision-btc"
            },
            "ci_usdt_zerod_bnb": {
                "wallet_id": settings.ci_usdt_zerod_bnb_wallet_id,
                "display_name": "CI-USDT-ZeroD-BNB"
            }
        }
    }