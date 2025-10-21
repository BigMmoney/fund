from fastapi import APIRouter, HTTPException, Query
from typing import Dict, List, Optional
from datetime import datetime, timedelta
import asyncio
from ...services.onetoken_client import onetoken_client

router = APIRouter()

@router.get("/portfolios")
async def get_all_portfolios():
    """获取所有投组及下属账户关系和投组本位币种"""
    try:
        # 获取投组列表
        portfolios_response = await onetoken_client.get_portfolios()
        
        # 获取账户列表及关系
        accounts_response = await onetoken_client.get_all_accounts()
        
        # 处理账户关系数据
        processed_data = onetoken_client.process_account_relationships(accounts_response)
        
        return {
            "status": "success",
            "portfolios_from_api": portfolios_response,
            "accounts_data": processed_data,
            "summary": {
                "total_portfolios": len(processed_data["portfolios"]),
                "total_accounts": len(processed_data["accounts"]),
                "total_relationships": len(processed_data["relationships"])
            }
        }
        
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"获取投组数据失败: {str(e)}")

@router.get("/accounts")
async def get_all_accounts():
    """获取所有账户信息"""
    try:
        # 获取账户列表
        accounts_response = await onetoken_client.get_all_accounts()
        
        # 处理账户关系数据
        processed_data = onetoken_client.process_account_relationships(accounts_response)
        
        return {
            "status": "success",
            "data": processed_data["accounts"],
            "count": len(processed_data["accounts"])
        }
        
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"获取账户数据失败: {str(e)}")

@router.get("/portfolio/{portfolio_name}/nav")
async def get_portfolio_nav(
    portfolio_name: str,
    start_time: Optional[str] = Query(None, description="开始时间，格式: YYYY-MM-DD HH:MM:SS"),
    end_time: Optional[str] = Query(None, description="结束时间，格式: YYYY-MM-DD HH:MM:SS"),
    hours: Optional[int] = Query(24, description="获取最近N小时的数据")
):
    """获取投组净值历史数据"""
    try:
        # 如果没有指定时间，使用默认的24小时
        if not start_time or not end_time:
            end_dt = datetime.now()
            start_dt = end_dt - timedelta(hours=hours)
            start_time = start_dt.strftime("%Y-%m-%d %H:%M:%S")
            end_time = end_dt.strftime("%Y-%m-%d %H:%M:%S")
        
        # 获取投组净值数据
        nav_data = await onetoken_client.get_portfolio_nav_history(
            portfolio_name, start_time, end_time
        )
        
        return {
            "status": "success",
            "portfolio_name": portfolio_name,
            "start_time": start_time,
            "end_time": end_time,
            "data": nav_data
        }
        
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"获取投组净值数据失败: {str(e)}")

@router.get("/account/{account_symbol}/nav")
async def get_account_nav(
    account_symbol: str,
    start_time: Optional[str] = Query(None, description="开始时间，格式: YYYY-MM-DD HH:MM:SS"),
    end_time: Optional[str] = Query(None, description="结束时间，格式: YYYY-MM-DD HH:MM:SS"),
    hours: Optional[int] = Query(24, description="获取最近N小时的数据")
):
    """获取账户净值历史数据"""
    try:
        # 如果没有指定时间，使用默认的24小时
        if not start_time or not end_time:
            end_dt = datetime.now()
            start_dt = end_dt - timedelta(hours=hours)
            start_time = start_dt.strftime("%Y-%m-%d %H:%M:%S")
            end_time = end_dt.strftime("%Y-%m-%d %H:%M:%S")
        
        # 获取账户净值数据
        nav_data = await onetoken_client.get_account_nav_history(
            account_symbol, start_time, end_time
        )
        
        return {
            "status": "success",
            "account_symbol": account_symbol,
            "start_time": start_time,
            "end_time": end_time,
            "data": nav_data
        }
        
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"获取账户净值数据失败: {str(e)}")

@router.get("/account/{account_symbol}/pnl")
async def calculate_account_pnl(
    account_symbol: str,
    start_time: Optional[str] = Query(None, description="开始时间，格式: YYYY-MM-DD HH:MM:SS"),
    end_time: Optional[str] = Query(None, description="结束时间，格式: YYYY-MM-DD HH:MM:SS"),
    hours: Optional[int] = Query(24, description="计算最近N小时的PnL")
):
    """计算指定时间区间账户PnL"""
    try:
        # 如果没有指定时间，使用默认的24小时
        if not start_time or not end_time:
            end_dt = datetime.now()
            start_dt = end_dt - timedelta(hours=hours)
            start_time = start_dt.strftime("%Y-%m-%d %H:%M:%S")
            end_time = end_dt.strftime("%Y-%m-%d %H:%M:%S")
        
        # 计算PnL
        pnl_data = await onetoken_client.calculate_pnl(
            account_symbol, start_time, end_time
        )
        
        return {
            "status": "success",
            "data": pnl_data
        }
        
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"计算PnL失败: {str(e)}")

@router.get("/relationships")
async def get_account_relationships():
    """获取账户关系映射"""
    try:
        # 获取账户列表
        accounts_response = await onetoken_client.get_all_accounts()
        
        # 处理账户关系数据
        processed_data = onetoken_client.process_account_relationships(accounts_response)
        
        return {
            "status": "success",
            "relationships": processed_data["relationships"],
            "count": len(processed_data["relationships"])
        }
        
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"获取账户关系失败: {str(e)}")

@router.get("/test/raw-accounts")
async def test_raw_accounts():
    """测试接口：获取原始账户数据"""
    try:
        # 获取原始账户数据
        accounts_response = await onetoken_client.get_all_accounts()
        
        return {
            "status": "success",
            "raw_data": accounts_response
        }
        
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"获取原始账户数据失败: {str(e)}")

@router.get("/test/raw-portfolios")
async def test_raw_portfolios():
    """测试接口：获取原始投组数据"""
    try:
        # 获取原始投组数据
        portfolios_response = await onetoken_client.get_portfolios()
        
        return {
            "status": "success",
            "raw_data": portfolios_response
        }
        
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"获取原始投组数据失败: {str(e)}")