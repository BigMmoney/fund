from fastapi import APIRouter, HTTPException, Query
from typing import Dict, Any, List, Optional
from ...services.onetoken_client import onetoken_client
from pydantic import BaseModel
import time
import logging

logger = logging.getLogger(__name__)

router = APIRouter(prefix="/api/onetoken", tags=["OneToken"])

class NAVRequest(BaseModel):
    """NAV请求模型"""
    portfolio_name: str
    start_time: int  # 纳秒时间戳
    end_time: int  # 纳秒时间戳
    frequency: str = "hourly"  # hourly 或 daily

@router.get("/test")
async def test_onetoken_connection():
    """测试OneToken连接"""
    if not onetoken_client:
        raise HTTPException(status_code=500, detail="OneToken客户端未初始化")
    
    try:
        # 测试ping端点
        result = onetoken_client._make_request("GET", "/api/v1/httpmisc/ping")
        return {"success": True, "data": result, "message": "OneToken连接成功"}
    except Exception as e:
        logger.error(f"OneToken连接失败: {e}")
        raise HTTPException(status_code=500, detail=f"OneToken连接失败: {str(e)}")

@router.get("/portfolios")
async def get_portfolios():
    """获取所有投资组合列表"""
    if not onetoken_client:
        raise HTTPException(status_code=500, detail="OneToken客户端未初始化")
    
    try:
        result = onetoken_client.get_portfolios()
        
        # 检查是否有错误
        if "error" in result:
            raise HTTPException(status_code=500, detail=result["error"])
        
        # 处理数据
        processed_result = onetoken_client.process_portfolio_data(result)
        return {"success": True, "data": processed_result}
    except HTTPException:
        raise
    except Exception as e:
        logger.error(f"获取投资组合失败: {e}")
        raise HTTPException(status_code=500, detail=f"获取投资组合失败: {str(e)}")

@router.get("/portfolios/{portfolio_name:path}")
async def get_portfolio_detail(portfolio_name: str):
    """获取投资组合详情（包含子账户）"""
    if not onetoken_client:
        raise HTTPException(status_code=500, detail="OneToken客户端未初始化")
    
    try:
        # portfolio_name 可能包含斜杠，如 fund/cendmz
        result = onetoken_client.get_portfolio_detail(portfolio_name)
        
        if "error" in result:
            raise HTTPException(status_code=500, detail=result["error"])
        
        return {"success": True, "data": result}
    except HTTPException:
        raise
    except Exception as e:
        logger.error(f"获取投资组合详情失败: {e}")
        raise HTTPException(status_code=500, detail=f"获取投资组合详情失败: {str(e)}")

@router.post("/nav")
async def get_historical_nav(request: NAVRequest):
    """获取历史NAV数据"""
    if not onetoken_client:
        raise HTTPException(status_code=500, detail="OneToken客户端未初始化")
    
    try:
        result = onetoken_client.get_portfolio_historical_nav(
            request.portfolio_name,
            request.start_time,
            request.end_time,
            request.frequency
        )
        
        if "error" in result:
            raise HTTPException(status_code=500, detail=result["error"])
        
        return {"success": True, "data": result}
    except HTTPException:
        raise
    except Exception as e:
        logger.error(f"获取历史NAV失败: {e}")
        raise HTTPException(status_code=500, detail=f"获取历史NAV失败: {str(e)}")

@router.get("/portfolios/{portfolio_name:path}/pnl/24h")
async def get_24h_pnl(portfolio_name: str):
    """获取24小时PnL"""
    if not onetoken_client:
        raise HTTPException(status_code=500, detail="OneToken客户端未初始化")
    
    try:
        result = onetoken_client.get_24h_portfolio_pnl(portfolio_name)
        
        if "error" in result:
            raise HTTPException(status_code=500, detail=result["error"])
        
        return {"success": True, "data": result}
    except HTTPException:
        raise
    except Exception as e:
        logger.error(f"获取24小时PnL失败: {e}")
        raise HTTPException(status_code=500, detail=f"获取24小时PnL失败: {str(e)}")

@router.get("/accounts")
async def get_accounts():
    """获取所有账户"""
    if not onetoken_client:
        raise HTTPException(status_code=500, detail="OneToken客户端未初始化")
    
    try:
        result = onetoken_client.get_all_accounts()
        
        if "error" in result:
            raise HTTPException(status_code=500, detail=result["error"])
        
        return {"success": True, "data": result}
    except HTTPException:
        raise
    except Exception as e:
        logger.error(f"获取账户失败: {e}")
        raise HTTPException(status_code=500, detail=f"获取账户失败: {str(e)}")

@router.get("/portfolios-with-accounts")
async def get_portfolios_with_accounts():
    """获取所有投资组合及其账户关系"""
    if not onetoken_client:
        raise HTTPException(status_code=500, detail="OneToken客户端未初始化")
    
    try:
        # 获取投资组合列表
        portfolios_result = onetoken_client.get_portfolios()
        
        # 获取账户列表  
        accounts_result = onetoken_client.get_all_accounts()
        
        # 处理数据
        processed_portfolios = onetoken_client.process_portfolio_data(portfolios_result)
        processed_accounts = onetoken_client.process_account_data(accounts_result)
        
        return {
            "success": True,
            "data": {
                "portfolios": processed_portfolios,
                "accounts": processed_accounts
            }
        }
    except Exception as e:
        logger.error(f"获取投资组合和账户关系失败: {e}")
        raise HTTPException(status_code=500, detail=f"获取投资组合和账户关系失败: {str(e)}")
