"""
OneToken 标准API实现
基于Onetoken.txt文档的完整API规范实现
支持所有标准端点和数据格式
"""

from fastapi import APIRouter, HTTPException, Query, Body
from pydantic import BaseModel, Field
from typing import Dict, Any, List, Optional, Union
from datetime import datetime
import time
import json
import logging

from ...services.onetoken_standard_client import OneTokenStandardClient
from server.app.onetoken_schemas import *

logger = logging.getLogger(__name__)
router = APIRouter()

# 初始化标准OneToken客户端
try:
    client = OneTokenStandardClient()
except Exception as e:
    logger.error(f"OneToken标准客户端初始化失败: {e}")
    client = None

# ==================== 认证和基础端点 ====================

@router.get("/health")
async def health_check():
    """健康检查端点"""
    return {
        "status": "ok",
        "service": "OneToken Standard API",
        "version": "1.0",
        "time": datetime.utcnow().isoformat(),
        "client_connected": client is not None
    }

@router.get("/ping")
async def ping():
    """Ping测试端点"""
    try:
        if not client:
            raise HTTPException(status_code=503, detail="OneToken客户端未初始化")
        
        # 使用标准ping端点
        result = await client.ping()
        return result
    except Exception as e:
        logger.error(f"Ping测试失败: {e}")
        raise HTTPException(status_code=500, detail=f"Ping测试失败: {str(e)}")

# ==================== 投资组合API ====================

@router.get("/fundv3/openapi/portfolio/list-portfolio", response_model=PortfolioListResponse)
async def list_portfolios():
    """
    获取投资组合列表
    完全兼容OneToken标准API格式
    """
    try:
        if not client:
            raise HTTPException(status_code=503, detail="OneToken客户端未初始化")
        
        result = await client.list_portfolios()
        
        # 转换为标准格式
        response = PortfolioListResponse(
            code="",
            message="success",
            request_time=int(time.time() * 1_000_000_000),
            response_time=int(time.time() * 1_000_000_000),
            result=PortfolioListResult(
                fund_info_list=[
                    PortfolioInfo(
                        fund_name=portfolio.get("fund_name", ""),
                        fund_alias=portfolio.get("fund_alias", ""),
                        denomination=portfolio.get("denomination", ""),
                        valuation_currency=portfolio.get("valuation_currency", ""),
                        status=portfolio.get("status", ""),
                        inception_time=portfolio.get("inception_time"),
                        inception_time_str=portfolio.get("inception_time_str", ""),
                        settlement_time=portfolio.get("settlement_time"),
                        settlement_time_str=portfolio.get("settlement_time_str", ""),
                        auto_ta_mode=portfolio.get("auto_ta_mode", False),
                        tag_list=portfolio.get("tag_list", []),
                        tag_alias_list=portfolio.get("tag_alias_list", []),
                        creation_time=portfolio.get("creation_time", 0),
                        creation_time_str=portfolio.get("creation_time_str", ""),
                        operator=portfolio.get("operator", ""),
                        operation_time=portfolio.get("operation_time", 0),
                        operation_time_str=portfolio.get("operation_time_str", ""),
                        version=portfolio.get("version", "v3"),
                        parent_fund_name=portfolio.get("parent_fund_name", ""),
                        parent_fund_alias=portfolio.get("parent_fund_alias", "")
                    )
                    for portfolio in result.get("fund_info_list", [])
                ]
            )
        )
        
        return response
        
    except Exception as e:
        logger.error(f"获取投资组合列表失败: {e}")
        return PortfolioListResponse(
            code="internal-error",
            message=f"获取投资组合列表失败: {str(e)}",
            request_time=int(time.time() * 1_000_000_000),
            response_time=int(time.time() * 1_000_000_000),
            result=None
        )

@router.get("/fundv3/openapi/portfolio/get-portfolio-detail", response_model=PortfolioDetailResponse)
async def get_portfolio_detail(fund_name: str = Query(..., description="投资组合名称")):
    """
    获取投资组合详情
    包含子账户信息和关系
    """
    try:
        if not client:
            raise HTTPException(status_code=503, detail="OneToken客户端未初始化")
        
        result = await client.get_portfolio_detail(fund_name)
        
        # 转换为标准格式
        response = PortfolioDetailResponse(
            code="",
            message="success",
            request_time=int(time.time() * 1_000_000_000),
            response_time=int(time.time() * 1_000_000_000),
            result=PortfolioDetailResult(
                fund_name=result.get("fund_name", ""),
                fund_alias=result.get("fund_alias", ""),
                denomination=result.get("denomination", ""),
                valuation_currency=result.get("valuation_currency", ""),
                status=result.get("status", ""),
                inception_time=result.get("inception_time"),
                creation_time=result.get("creation_time", 0),
                operator=result.get("operator", ""),
                auto_ta_mode=result.get("auto_ta_mode", False),
                version=result.get("version", "v3"),
                fund_children=[
                    ChildAccount(
                        child_name=child.get("child_name", ""),
                        child_alias=child.get("child_alias", ""),
                        child_type=child.get("child_type", ""),
                        venue=child.get("venue", ""),
                        account_mode=child.get("account_mode", ""),
                        net_assets_usd=child.get("net_assets_usd", "0"),
                        status=child.get("status", "")
                    )
                    for child in result.get("fund_children", [])
                ]
            )
        )
        
        return response
        
    except Exception as e:
        logger.error(f"获取投资组合详情失败: {e}")
        return PortfolioDetailResponse(
            code="fund-not-found" if "not found" in str(e).lower() else "internal-error",
            message=f"获取投资组合详情失败: {str(e)}",
            request_time=int(time.time() * 1_000_000_000),
            response_time=int(time.time() * 1_000_000_000),
            result=None
        )

@router.post("/fundv3/openapi/portfolio/get-historical-nav", response_model=HistoricalNavResponse)
async def get_historical_nav(request: HistoricalNavRequest):
    """
    获取历史NAV数据
    支持daily和hourly频率
    """
    try:
        if not client:
            raise HTTPException(status_code=503, detail="OneToken客户端未初始化")
        
        # 验证频率参数
        if request.frequency not in ["daily", "hourly"]:
            return HistoricalNavResponse(
                code="invalid-frequency",
                message="频率参数必须是 'daily' 或 'hourly'",
                request_time=int(time.time() * 1_000_000_000),
                response_time=int(time.time() * 1_000_000_000),
                result=None
            )
        
        result = await client.get_historical_nav(
            fund_name=request.fund_name,
            start_time=request.start_time,
            end_time=request.end_time,
            frequency=request.frequency
        )
        
        # 转换为标准格式
        response = HistoricalNavResponse(
            code="",
            message="success",
            request_time=int(time.time() * 1_000_000_000),
            response_time=int(time.time() * 1_000_000_000),
            result=HistoricalNavResult(
                historical_nav=[
                    NavDataPoint(
                        fund_name=nav.get("fund_name", ""),
                        fund_alias=nav.get("fund_alias", ""),
                        valuation_currency=nav.get("valuation_currency", ""),
                        snapshot_time=nav.get("snapshot_time", 0),
                        snapshot_time_str=nav.get("snapshot_time_str", ""),
                        net_assets=nav.get("net_assets", "0"),
                        net_assets_str=nav.get("net_assets_str", "0"),
                        accum_nav=nav.get("accum_nav", "1"),
                        accum_nav_str=nav.get("accum_nav_str", "1"),
                        accum_pnl=nav.get("accum_pnl", "0"),
                        accum_pnl_str=nav.get("accum_pnl_str", "0")
                    )
                    for nav in result.get("historical_nav", [])
                ]
            )
        )
        
        return response
        
    except Exception as e:
        logger.error(f"获取历史NAV数据失败: {e}")
        return HistoricalNavResponse(
            code="fund-not-found" if "not found" in str(e).lower() else "internal-error",
            message=f"获取历史NAV数据失败: {str(e)}",
            request_time=int(time.time() * 1_000_000_000),
            response_time=int(time.time() * 1_000_000_000),
            result=None
        )

# ==================== 便利端点（简化调用） ====================

@router.get("/portfolios")
async def get_portfolios_simple():
    """
    简化的投资组合列表端点
    返回简化格式，便于前端调用
    """
    try:
        if not client:
            raise HTTPException(status_code=503, detail="OneToken客户端未初始化")
        
        result = await client.list_portfolios()
        
        # 简化格式
        simplified = {
            "success": True,
            "total": len(result.get("fund_info_list", [])),
            "portfolios": [
                {
                    "id": portfolio.get("fund_name", ""),
                    "name": portfolio.get("fund_alias", ""),
                    "currency": portfolio.get("denomination", ""),
                    "status": portfolio.get("status", ""),
                    "created": portfolio.get("creation_time_str", ""),
                    "operator": portfolio.get("operator", "")
                }
                for portfolio in result.get("fund_info_list", [])
            ]
        }
        
        return simplified
        
    except Exception as e:
        logger.error(f"获取简化投资组合列表失败: {e}")
        return {
            "success": False,
            "error": str(e),
            "total": 0,
            "portfolios": []
        }

@router.get("/portfolios/{fund_name}/nav")
async def get_portfolio_nav_simple(
    fund_name: str,
    frequency: str = Query("daily", pattern="^(daily|hourly)$", description="数据频率"),
    days: int = Query(7, ge=1, le=365, description="查询天数")
):
    """
    简化的NAV数据端点
    自动计算时间范围
    """
    try:
        if not client:
            raise HTTPException(status_code=503, detail="OneToken客户端未初始化")
        
        # 计算时间范围（纳秒时间戳）
        end_time_ns = int(time.time() * 1_000_000_000)
        start_time_ns = end_time_ns - (days * 24 * 60 * 60 * 1_000_000_000)
        
        result = await client.get_historical_nav(
            fund_name=fund_name,
            start_time=start_time_ns,
            end_time=end_time_ns,
            frequency=frequency
        )
        
        nav_data = result.get("historical_nav", [])
        
        # 计算统计信息
        if len(nav_data) >= 2:
            latest = nav_data[-1]
            first = nav_data[0]
            
            period_return = (
                float(latest.get("accum_nav", "1")) / float(first.get("accum_nav", "1")) - 1
            ) * 100 if float(first.get("accum_nav", "1")) > 0 else 0
            
            period_pnl = (
                float(latest.get("accum_pnl", "0")) - float(first.get("accum_pnl", "0"))
            )
        else:
            period_return = 0
            period_pnl = 0
        
        simplified = {
            "success": True,
            "fund_info": {
                "fund_name": fund_name,
                "fund_alias": nav_data[0].get("fund_alias", "") if nav_data else "",
                "currency": nav_data[0].get("valuation_currency", "") if nav_data else ""
            },
            "nav_data": [
                {
                    "timestamp": nav.get("snapshot_time", 0),
                    "time_str": nav.get("snapshot_time_str", ""),
                    "nav": float(nav.get("accum_nav", "1")),
                    "net_assets": float(nav.get("net_assets", "0")),
                    "pnl": float(nav.get("accum_pnl", "0"))
                }
                for nav in nav_data
            ],
            "statistics": {
                "total_points": len(nav_data),
                "period_return_pct": round(period_return, 4),
                "period_pnl": round(period_pnl, 6),
                "frequency": frequency,
                "days": days
            }
        }
        
        return simplified
        
    except Exception as e:
        logger.error(f"获取简化NAV数据失败: {e}")
        return {
            "success": False,
            "error": str(e),
            "fund_info": None,
            "nav_data": [],
            "statistics": None
        }

# ==================== 高级分析端点 ====================

@router.get("/portfolios/{fund_name}/performance")
async def get_portfolio_performance(
    fund_name: str,
    period: str = Query("7d", pattern="^(1d|7d|30d|90d|1y)$", description="分析周期")
):
    """
    投资组合绩效分析
    计算收益率、波动率、最大回撤等指标
    """
    try:
        if not client:
            raise HTTPException(status_code=503, detail="OneToken客户端未初始化")
        
        # 根据周期确定天数
        period_days = {
            "1d": 1, "7d": 7, "30d": 30, "90d": 90, "1y": 365
        }
        days = period_days.get(period, 7)
        
        # 获取NAV数据
        end_time_ns = int(time.time() * 1_000_000_000)
        start_time_ns = end_time_ns - (days * 24 * 60 * 60 * 1_000_000_000)
        
        result = await client.get_historical_nav(
            fund_name=fund_name,
            start_time=start_time_ns,
            end_time=end_time_ns,
            frequency="daily" if days > 7 else "hourly"
        )
        
        nav_data = result.get("historical_nav", [])
        
        if len(nav_data) < 2:
            return {
                "success": False,
                "error": "数据不足，无法计算绩效指标",
                "fund_name": fund_name
            }
        
        # 计算绩效指标
        navs = [float(point.get("accum_nav", "1")) for point in nav_data]
        returns = [(navs[i] / navs[i-1] - 1) for i in range(1, len(navs))]
        
        # 总收益率
        total_return = (navs[-1] / navs[0] - 1) * 100
        
        # 年化收益率
        years = days / 365
        annualized_return = ((navs[-1] / navs[0]) ** (1/years) - 1) * 100 if years > 0 else 0
        
        # 波动率
        if len(returns) > 1:
            mean_return = sum(returns) / len(returns)
            variance = sum((r - mean_return) ** 2 for r in returns) / (len(returns) - 1)
            volatility = (variance ** 0.5) * (365 ** 0.5) * 100  # 年化波动率
        else:
            volatility = 0
        
        # 最大回撤
        peak = navs[0]
        max_drawdown = 0
        for nav in navs:
            if nav > peak:
                peak = nav
            drawdown = (peak - nav) / peak
            if drawdown > max_drawdown:
                max_drawdown = drawdown
        max_drawdown *= 100
        
        # 夏普比率 (假设无风险利率为0)
        sharpe_ratio = (annualized_return / volatility) if volatility > 0 else 0
        
        performance = {
            "success": True,
            "fund_info": {
                "fund_name": fund_name,
                "fund_alias": nav_data[0].get("fund_alias", ""),
                "currency": nav_data[0].get("valuation_currency", "")
            },
            "period": {
                "days": days,
                "period_code": period,
                "start_time": nav_data[0].get("snapshot_time_str", ""),
                "end_time": nav_data[-1].get("snapshot_time_str", "")
            },
            "performance_metrics": {
                "total_return_pct": round(total_return, 4),
                "annualized_return_pct": round(annualized_return, 4),
                "volatility_pct": round(volatility, 4),
                "max_drawdown_pct": round(max_drawdown, 4),
                "sharpe_ratio": round(sharpe_ratio, 4),
                "data_points": len(nav_data)
            },
            "current_values": {
                "current_nav": navs[-1],
                "current_pnl": float(nav_data[-1].get("accum_pnl", "0")),
                "net_assets": float(nav_data[-1].get("net_assets", "0"))
            }
        }
        
        return performance
        
    except Exception as e:
        logger.error(f"获取投资组合绩效分析失败: {e}")
        return {
            "success": False,
            "error": str(e),
            "fund_name": fund_name
        }

@router.get("/analytics/portfolio-comparison")
async def compare_portfolios(
    fund_names: str = Query(..., description="投资组合名称，逗号分隔"),
    period: str = Query("30d", pattern="^(7d|30d|90d)$", description="对比周期")
):
    """
    多投资组合对比分析
    """
    try:
        if not client:
            raise HTTPException(status_code=503, detail="OneToken客户端未初始化")
        
        portfolio_names = [name.strip() for name in fund_names.split(",")]
        period_days = {"7d": 7, "30d": 30, "90d": 90}
        days = period_days.get(period, 30)
        
        comparison_data = []
        
        for fund_name in portfolio_names:
            try:
                # 获取单个投资组合的绩效数据
                perf_response = await get_portfolio_performance(fund_name, period)
                if perf_response.get("success"):
                    comparison_data.append({
                        "fund_name": fund_name,
                        "fund_alias": perf_response["fund_info"]["fund_alias"],
                        "metrics": perf_response["performance_metrics"]
                    })
            except:
                continue
        
        return {
            "success": True,
            "comparison_period": period,
            "portfolios_count": len(comparison_data),
            "portfolios": comparison_data
        }
        
    except Exception as e:
        logger.error(f"投资组合对比分析失败: {e}")
        return {
            "success": False,
            "error": str(e)
        }