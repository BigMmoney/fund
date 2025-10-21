from fastapi import APIRouter, HTTPException, Depends, Query
from typing import Dict, Any, List, Optional
from datetime import datetime, timedelta
from ...services.onetoken_client import OneTokenClient
from ...db.mysql import execute_query, get_pool
from server.app.settings import settings
import json
import time
import asyncio

router = APIRouter()

def get_onetoken_client() -> OneTokenClient:
    """获取1Token客户端实例"""
    return OneTokenClient()

@router.get("/field-mapping/accounts")
async def get_account_field_mapping():
    """获取账户字段映射和说明"""
    return {
        "fields": {
            "account": {
                "description": "账户信息",
                "type": "object",
                "fields": {
                    "alias": {"description": "账户别名", "type": "string"},
                    "name": {"description": "账户完整名称", "type": "string"},
                    "is_root": {"description": "是否为根账户", "type": "boolean"},
                    "prop_exchange": {"description": "所属交易所", "type": "string"}
                }
            },
            "account_level": {"description": "账户级别代码", "type": "string"},
            "account_level_str": {"description": "账户级别描述", "type": "string"},
            "account_type": {"description": "账户类型代码", "type": "string"},
            "account_type_str": {"description": "账户类型描述", "type": "string"},
            "balance": {"description": "账户余额", "type": "decimal"},
            "balance_translate": {"description": "余额折算值", "type": "decimal"},
            "balance_usdt_translate": {"description": "USDT折算余额", "type": "decimal"},
            "balance_btc_translate": {"description": "BTC折算余额", "type": "decimal"},
            "api_status": {"description": "API状态代码", "type": "string"},
            "api_status_str": {"description": "API状态描述", "type": "string"},
            "api_error_message": {"description": "API错误信息", "type": "string"},
            "api_check_time": {"description": "API检查时间戳", "type": "long"},
            "api_check_time_str": {"description": "API检查时间字符串", "type": "string"},
            "create_time": {"description": "创建时间戳", "type": "long"},
            "create_time_str": {"description": "创建时间字符串", "type": "string"}
        }
    }

@router.get("/field-mapping/portfolios")
async def get_portfolio_field_mapping():
    """获取投资组合字段映射和说明"""
    return {
        "fields": {
            "fund_name": {"description": "基金名称", "type": "string"},
            "fund_display_name": {"description": "基金显示名称", "type": "string"},
            "fund_type": {"description": "基金类型", "type": "string"},
            "fund_type_str": {"description": "基金类型描述", "type": "string"},
            "fund_status": {"description": "基金状态代码", "type": "string"},
            "fund_status_str": {"description": "基金状态描述", "type": "string"},
            "nav": {"description": "净值", "type": "decimal"},
            "nav_str": {"description": "净值字符串", "type": "string"},
            "aum": {"description": "管理资产规模", "type": "decimal"},
            "aum_str": {"description": "管理资产规模字符串", "type": "string"},
            "daily_pnl": {"description": "日收益", "type": "decimal"},
            "daily_pnl_str": {"description": "日收益字符串", "type": "string"},
            "daily_pnl_rate": {"description": "日收益率", "type": "decimal"},
            "daily_pnl_rate_str": {"description": "日收益率字符串", "type": "string"},
            "create_time": {"description": "创建时间戳", "type": "long"},
            "create_time_str": {"description": "创建时间字符串", "type": "string"}
        }
    }

@router.get("/formatted-data/accounts")
async def get_formatted_accounts():
    """获取格式化的账户数据（适合前端展示）"""
    client = get_onetoken_client()
    raw_data = await client.list_all_accounts()
    
    if not raw_data or 'account_list' not in raw_data:
        raise HTTPException(status_code=404, detail="无账户数据")
    
    # 格式化数据，提供前端友好的结构
    formatted_accounts = []
    for account in raw_data['account_list']:
        formatted = {
            "id": account.get("account", {}).get("name", ""),
            "display_name": account.get("account", {}).get("alias", ""),
            "exchange": account.get("account", {}).get("prop_exchange", ""),
            "level": account.get("account_level_str", ""),
            "type": account.get("account_type_str", ""),
            "status": {
                "code": account.get("api_status", ""),
                "description": account.get("api_status_str", ""),
                "is_active": account.get("api_status") == "api-success"
            },
            "balance": {
                "amount": float(account.get("balance", 0)),
                "usdt_value": float(account.get("balance_usdt_translate", 0)),
                "btc_value": float(account.get("balance_btc_translate", 0)),
                "currency": account.get("asset_base", "")
            },
            "last_check": {
                "timestamp": account.get("api_check_time", 0),
                "datetime": account.get("api_check_time_str", ""),
                "error_message": account.get("api_error_message", "")
            },
            "is_root": account.get("account", {}).get("is_root", False),
            "raw_data": account  # 保留原始数据供高级用户使用
        }
        formatted_accounts.append(formatted)
    
    return {
        "total": len(formatted_accounts),
        "accounts": formatted_accounts,
        "summary": {
            "total_usdt_value": sum(acc["balance"]["usdt_value"] for acc in formatted_accounts),
            "active_accounts": len([acc for acc in formatted_accounts if acc["status"]["is_active"]]),
            "error_accounts": len([acc for acc in formatted_accounts if not acc["status"]["is_active"]])
        }
    }

@router.get("/formatted-data/portfolios")
async def get_formatted_portfolios():
    """获取格式化的投资组合数据（适合前端展示）"""
    client = get_onetoken_client()
    raw_data = await client.list_all_portfolios()
    
    if not raw_data or 'fund_info_list' not in raw_data:
        raise HTTPException(status_code=404, detail="无投资组合数据")
    
    # 格式化数据
    formatted_portfolios = []
    for fund in raw_data['fund_info_list']:
        formatted = {
            "id": fund.get("fund_name", ""),
            "display_name": fund.get("fund_display_name", ""),
            "type": {
                "code": fund.get("fund_type", ""),
                "description": fund.get("fund_type_str", "")
            },
            "status": {
                "code": fund.get("fund_status", ""),
                "description": fund.get("fund_status_str", ""),
                "is_active": fund.get("fund_status") == "active"
            },
            "performance": {
                "nav": float(fund.get("nav", 0)),
                "aum": float(fund.get("aum", 0)),
                "daily_pnl": float(fund.get("daily_pnl", 0)),
                "daily_pnl_rate": float(fund.get("daily_pnl_rate", 0))
            },
            "created": {
                "timestamp": fund.get("create_time", 0),
                "datetime": fund.get("create_time_str", "")
            },
            "raw_data": fund  # 保留原始数据
        }
        formatted_portfolios.append(formatted)
    
    return {
        "total": len(formatted_portfolios),
        "portfolios": formatted_portfolios,
        "summary": {
            "total_aum": sum(p["performance"]["aum"] for p in formatted_portfolios),
            "total_daily_pnl": sum(p["performance"]["daily_pnl"] for p in formatted_portfolios),
            "active_portfolios": len([p for p in formatted_portfolios if p["status"]["is_active"]])
        }
    }

@router.get("/table-data/accounts")
async def get_accounts_table_data(
    page: int = Query(1, ge=1, description="页码"),
    size: int = Query(20, ge=1, le=100, description="每页数量"),
    search: Optional[str] = Query(None, description="搜索关键词"),
    status_filter: Optional[str] = Query(None, description="状态筛选")
):
    """获取账户表格数据（分页、搜索、筛选）"""
    client = get_onetoken_client()
    raw_data = await client.list_all_accounts()
    
    if not raw_data or 'account_list' not in raw_data:
        return {"total": 0, "items": [], "page": page, "size": size}
    
    accounts = raw_data['account_list']
    
    # 应用搜索过滤
    if search:
        accounts = [
            acc for acc in accounts 
            if search.lower() in acc.get("account", {}).get("alias", "").lower() or
               search.lower() in acc.get("account", {}).get("name", "").lower()
        ]
    
    # 应用状态过滤
    if status_filter:
        accounts = [acc for acc in accounts if acc.get("api_status") == status_filter]
    
    total = len(accounts)
    
    # 分页
    start = (page - 1) * size
    end = start + size
    paged_accounts = accounts[start:end]
    
    # 格式化为表格数据
    table_items = []
    for account in paged_accounts:
        item = {
            "account_name": account.get("account", {}).get("alias", ""),
            "exchange": account.get("account", {}).get("prop_exchange", ""),
            "type": account.get("account_type_str", ""),
            "balance_usdt": float(account.get("balance_usdt_translate", 0)),
            "status": account.get("api_status_str", ""),
            "last_check": account.get("api_check_time_str", ""),
            "is_active": account.get("api_status") == "api-success"
        }
        table_items.append(item)
    
    return {
        "total": total,
        "items": table_items,
        "page": page,
        "size": size,
        "total_pages": (total + size - 1) // size
    }

@router.get("/table-data/portfolios")
async def get_portfolios_table_data(
    page: int = Query(1, ge=1, description="页码"),
    size: int = Query(20, ge=1, le=100, description="每页数量"),
    search: Optional[str] = Query(None, description="搜索关键词")
):
    """获取投资组合表格数据（分页、搜索）"""
    client = get_onetoken_client()
    raw_data = await client.list_all_portfolios()
    
    if not raw_data or 'fund_info_list' not in raw_data:
        return {"total": 0, "items": [], "page": page, "size": size}
    
    portfolios = raw_data['fund_info_list']
    
    # 应用搜索过滤
    if search:
        portfolios = [
            p for p in portfolios 
            if search.lower() in p.get("fund_display_name", "").lower() or
               search.lower() in p.get("fund_name", "").lower()
        ]
    
    total = len(portfolios)
    
    # 分页
    start = (page - 1) * size
    end = start + size
    paged_portfolios = portfolios[start:end]
    
    # 格式化为表格数据
    table_items = []
    for portfolio in paged_portfolios:
        item = {
            "fund_name": portfolio.get("fund_display_name", ""),
            "type": portfolio.get("fund_type_str", ""),
            "nav": float(portfolio.get("nav", 0)),
            "aum": float(portfolio.get("aum", 0)),
            "daily_pnl": float(portfolio.get("daily_pnl", 0)),
            "daily_pnl_rate": float(portfolio.get("daily_pnl_rate", 0)),
            "status": portfolio.get("fund_status_str", ""),
            "is_active": portfolio.get("fund_status") == "active"
        }
        table_items.append(item)
    
    return {
        "total": total,
        "items": table_items,
        "page": page,
        "size": size,
        "total_pages": (total + size - 1) // size
    }

@router.get("/statistics/overview")
async def get_overview_statistics():
    """获取总览统计数据"""
    client = get_onetoken_client()
    
    # 并行获取数据
    import asyncio
    accounts_data, portfolios_data = await asyncio.gather(
        client.list_all_accounts(),
        client.list_all_portfolios()
    )
    
    stats = {
        "accounts": {
            "total": 0,
            "active": 0,
            "error": 0,
            "total_balance_usdt": 0.0
        },
        "portfolios": {
            "total": 0,
            "active": 0,
            "total_aum": 0.0,
            "total_daily_pnl": 0.0
        },
        "last_updated": datetime.now().isoformat()
    }
    
    # 处理账户统计
    if accounts_data and 'account_list' in accounts_data:
        accounts = accounts_data['account_list']
        stats["accounts"]["total"] = len(accounts)
        stats["accounts"]["active"] = len([a for a in accounts if a.get("api_status") == "api-success"])
        stats["accounts"]["error"] = stats["accounts"]["total"] - stats["accounts"]["active"]
        stats["accounts"]["total_balance_usdt"] = sum(
            float(a.get("balance_usdt_translate", 0)) for a in accounts
        )
    
    # 处理投资组合统计
    if portfolios_data and 'fund_info_list' in portfolios_data:
        portfolios = portfolios_data['fund_info_list']
        stats["portfolios"]["total"] = len(portfolios)
        stats["portfolios"]["active"] = len([p for p in portfolios if p.get("fund_status") == "active"])
        stats["portfolios"]["total_aum"] = sum(
            float(p.get("aum", 0)) for p in portfolios
        )
        stats["portfolios"]["total_daily_pnl"] = sum(
            float(p.get("daily_pnl", 0)) for p in portfolios
        )
    
    return stats

@router.get("/export/accounts")
async def export_accounts_data(format: str = Query("json", pattern="^(json|csv)$")):
    """导出账户数据"""
    client = get_onetoken_client()
    raw_data = await client.list_all_accounts()
    
    if not raw_data or 'account_list' not in raw_data:
        raise HTTPException(status_code=404, detail="无账户数据")
    
    if format == "csv":
        import csv
        import io
        
        # 创建CSV内容
        output = io.StringIO()
        writer = csv.writer(output)
        
        # 写入表头
        writer.writerow([
            "账户名称", "交易所", "账户类型", "状态", "余额(USDT)", 
            "余额(BTC)", "最后检查时间", "错误信息"
        ])
        
        # 写入数据
        for account in raw_data['account_list']:
            writer.writerow([
                account.get("account", {}).get("alias", ""),
                account.get("account", {}).get("prop_exchange", ""),
                account.get("account_type_str", ""),
                account.get("api_status_str", ""),
                account.get("balance_usdt_translate", "0"),
                account.get("balance_btc_translate", "0"),
                account.get("api_check_time_str", ""),
                account.get("api_error_message", "")
            ])
        
        csv_content = output.getvalue()
        output.close()
        
        return {
            "format": "csv",
            "content": csv_content,
            "filename": f"accounts_export_{datetime.now().strftime('%Y%m%d_%H%M%S')}.csv"
        }
    
    else:  # JSON格式
        return {
            "format": "json",
            "content": raw_data,
            "filename": f"accounts_export_{datetime.now().strftime('%Y%m%d_%H%M%S')}.json"
        }

@router.post("/database/save-snapshot")
async def save_data_snapshot():
    """保存当前数据快照到数据库"""
    client = get_onetoken_client()
    
    try:
        # 获取数据
        accounts_data, portfolios_data = await asyncio.gather(
            client.list_all_accounts(),
            client.list_all_portfolios()
        )
        
        # 计算总的NAV值和PnL
        total_nav = 0.0
        total_pnl = 0.0
        
        if portfolios_data and 'fund_info_list' in portfolios_data:
            for fund in portfolios_data['fund_info_list']:
                total_nav += float(fund.get('nav', 0))
                total_pnl += float(fund.get('daily_pnl', 0))
        
        # 保存到数据库
        pool = await get_pool()
        async with pool.acquire() as conn:
            async with conn.cursor() as cur:
                await cur.execute(
                    """
                    INSERT INTO fund_nav_snapshot (snapshot_time, nav_value, daily_pnl)
                    VALUES (%s, %s, %s)
                    """,
                    (datetime.now(), total_nav, total_pnl)
                )
        
        return {
            "success": True,
            "message": "数据快照已保存",
            "snapshot_time": datetime.now().isoformat(),
            "nav_value": total_nav,
            "daily_pnl": total_pnl
        }
        
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"保存快照失败: {str(e)}")

@router.get("/database/snapshots")
async def get_nav_snapshots(
    days: int = Query(7, ge=1, le=365, description="获取最近几天的数据")
):
    """获取NAV快照历史数据"""
    
    try:
        start_date = datetime.now() - timedelta(days=days)
        
        results = await execute_query(
            """
            SELECT snapshot_time, nav_value, daily_pnl
            FROM fund_nav_snapshot
            WHERE snapshot_time >= %s
            ORDER BY snapshot_time DESC
            """,
            (start_date,)
        )
        
        return {
            "total": len(results),
            "snapshots": [
                {
                    "time": row[0].isoformat(),
                    "nav": float(row[1]),
                    "pnl": float(row[2])
                }
                for row in results
            ]
        }
        
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"获取快照数据失败: {str(e)}")

# ==================== 投资组合相关API ====================

@router.get("/portfolios/list")
async def get_all_portfolios(client: OneTokenClient = Depends(get_onetoken_client)):
    """获取所有投资组合列表"""
    try:
        result = await client.list_all_portfolios()
        if result is None:
            raise HTTPException(status_code=503, detail="OneToken API 连接失败")
        
        portfolios = result.get('portfolio_list', [])
        
        # 整理返回数据
        formatted_portfolios = []
        for portfolio in portfolios:
            formatted_portfolios.append({
                "portfolio_id": portfolio.get('portfolio_id'),
                "portfolio_name": portfolio.get('portfolio_name'),
                "base_currency": portfolio.get('base_currency'),
                "status": portfolio.get('status'),
                "total_nav": portfolio.get('total_nav'),
                "created_at": portfolio.get('created_at'),
                "updated_at": portfolio.get('updated_at')
            })
        
        return {
            "success": True,
            "total": len(formatted_portfolios),
            "portfolios": formatted_portfolios
        }
        
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"获取投资组合列表失败: {str(e)}")

@router.get("/portfolios/{portfolio_id}/detail")
async def get_portfolio_detail(
    portfolio_id: str,
    client: OneTokenClient = Depends(get_onetoken_client)
):
    """获取投资组合详情，包括下属账户关系"""
    try:
        result = await client.get_portfolio_detail(portfolio_id)
        if result is None:
            raise HTTPException(status_code=404, detail=f"投资组合 {portfolio_id} 不存在或无权访问")
        
        # 整理账户信息
        accounts = result.get('account_list', [])
        formatted_accounts = []
        
        for account in accounts:
            formatted_accounts.append({
                "account_id": account.get('account'),
                "exchange": account.get('exchange'),
                "account_type": account.get('account_type'),
                "status": account.get('status'),
                "nav": account.get('nav'),
                "currency": account.get('currency'),
                "created_at": account.get('created_at')
            })
        
        return {
            "success": True,
            "portfolio_info": {
                "portfolio_id": result.get('portfolio_id'),
                "portfolio_name": result.get('portfolio_name'),
                "base_currency": result.get('base_currency'),
                "total_nav": result.get('total_nav'),
                "status": result.get('status'),
                "created_at": result.get('created_at'),
                "updated_at": result.get('updated_at')
            },
            "accounts": {
                "total": len(formatted_accounts),
                "account_list": formatted_accounts
            }
        }
        
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"获取投资组合详情失败: {str(e)}")

@router.get("/portfolios/{portfolio_id}/nav/historical")
async def get_historical_nav(
    portfolio_id: str,
    start_time: Optional[int] = Query(None, description="开始时间戳，默认24小时前"),
    end_time: Optional[int] = Query(None, description="结束时间戳，默认当前时间"),
    frequency: str = Query("hourly", pattern="^(hourly|daily)$", description="数据频率"),
    client: OneTokenClient = Depends(get_onetoken_client)
):
    """获取投资组合历史净资产"""
    try:
        # 设置默认时间范围
        if end_time is None:
            end_time = int(time.time())
        if start_time is None:
            start_time = end_time - 24 * 3600  # 24小时前
        
        result = await client.get_historical_nav(
            portfolio_id=portfolio_id,
            start_time=start_time,
            end_time=end_time,
            frequency=frequency
        )
        
        if result is None:
            raise HTTPException(status_code=404, detail=f"投资组合 {portfolio_id} 的历史数据不存在")
        
        nav_list = result.get('nav_list', [])
        
        # 计算PnL统计
        pnl_analysis = {}
        if len(nav_list) >= 2:
            first_nav = nav_list[0].get('nav', 0)
            last_nav = nav_list[-1].get('nav', 0)
            
            # 24小时PnL
            pnl_24h = last_nav - first_nav
            pnl_rate_24h = (pnl_24h / first_nav * 100) if first_nav > 0 else 0
            
            # 最近1小时PnL（如果有足够数据）
            pnl_1h = 0
            pnl_rate_1h = 0
            if len(nav_list) >= 2:
                prev_nav = nav_list[-2].get('nav', 0)
                pnl_1h = last_nav - prev_nav
                pnl_rate_1h = (pnl_1h / prev_nav * 100) if prev_nav > 0 else 0
            
            # 找出最高和最低点
            max_nav_point = max(nav_list, key=lambda x: x.get('nav', 0))
            min_nav_point = min(nav_list, key=lambda x: x.get('nav', 0))
            
            pnl_analysis = {
                "period_pnl": {
                    "start_nav": first_nav,
                    "end_nav": last_nav,
                    "absolute_pnl": pnl_24h,
                    "pnl_rate_percent": pnl_rate_24h
                },
                "recent_1h_pnl": {
                    "absolute_pnl": pnl_1h,
                    "pnl_rate_percent": pnl_rate_1h
                },
                "extremes": {
                    "max_nav": max_nav_point.get('nav', 0),
                    "max_nav_time": max_nav_point.get('timestamp', 0),
                    "min_nav": min_nav_point.get('nav', 0),
                    "min_nav_time": min_nav_point.get('timestamp', 0),
                    "volatility_percent": ((max_nav_point.get('nav', 0) - min_nav_point.get('nav', 0)) / first_nav * 100) if first_nav > 0 else 0
                }
            }
        
        return {
            "success": True,
            "portfolio_id": portfolio_id,
            "time_range": {
                "start_time": start_time,
                "end_time": end_time,
                "start_time_str": datetime.fromtimestamp(start_time).strftime('%Y-%m-%d %H:%M:%S'),
                "end_time_str": datetime.fromtimestamp(end_time).strftime('%Y-%m-%d %H:%M:%S'),
                "frequency": frequency
            },
            "nav_data": {
                "total_points": len(nav_list),
                "nav_list": [
                    {
                        "timestamp": nav_point.get('timestamp'),
                        "time_str": datetime.fromtimestamp(nav_point.get('timestamp', 0)).strftime('%Y-%m-%d %H:%M:%S'),
                        "nav": nav_point.get('nav')
                    }
                    for nav_point in nav_list
                ]
            },
            "pnl_analysis": pnl_analysis
        }
        
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"获取历史净资产失败: {str(e)}")

@router.get("/portfolios/summary")
async def get_portfolios_summary(client: OneTokenClient = Depends(get_onetoken_client)):
    """获取所有投资组合的汇总信息"""
    try:
        # 获取投资组合列表
        portfolio_result = await client.list_all_portfolios()
        if portfolio_result is None:
            raise HTTPException(status_code=503, detail="OneToken API 连接失败")
        
        portfolios = portfolio_result.get('portfolio_list', [])
        
        # 统计信息
        total_portfolios = len(portfolios)
        currency_stats = {}
        total_nav_by_currency = {}
        status_stats = {}
        
        for portfolio in portfolios:
            currency = portfolio.get('base_currency', 'Unknown')
            status = portfolio.get('status', 'Unknown')
            nav = portfolio.get('total_nav', 0)
            
            # 按币种统计
            currency_stats[currency] = currency_stats.get(currency, 0) + 1
            total_nav_by_currency[currency] = total_nav_by_currency.get(currency, 0) + nav
            
            # 按状态统计
            status_stats[status] = status_stats.get(status, 0) + 1
        
        return {
            "success": True,
            "summary": {
                "total_portfolios": total_portfolios,
                "currency_distribution": [
                    {
                        "currency": currency,
                        "count": count,
                        "total_nav": total_nav_by_currency.get(currency, 0)
                    }
                    for currency, count in currency_stats.items()
                ],
                "status_distribution": [
                    {
                        "status": status,
                        "count": count
                    }
                    for status, count in status_stats.items()
                ]
            },
            "portfolios": [
                {
                    "portfolio_id": p.get('portfolio_id'),
                    "portfolio_name": p.get('portfolio_name'),
                    "base_currency": p.get('base_currency'),
                    "total_nav": p.get('total_nav'),
                    "status": p.get('status')
                }
                for p in portfolios
            ]
        }
        
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"获取投资组合汇总失败: {str(e)}")

@router.post("/portfolios/batch/nav")
async def get_batch_portfolio_nav(
    portfolio_ids: List[str],
    start_time: Optional[int] = None,
    end_time: Optional[int] = None,
    frequency: str = "hourly",
    client: OneTokenClient = Depends(get_onetoken_client)
):
    """批量获取多个投资组合的历史净资产"""
    try:
        # 设置默认时间范围
        if end_time is None:
            end_time = int(time.time())
        if start_time is None:
            start_time = end_time - 24 * 3600  # 24小时前
        
        # 并发获取多个投资组合的数据
        tasks = []
        for portfolio_id in portfolio_ids:
            tasks.append(
                client.get_historical_nav(
                    portfolio_id=portfolio_id,
                    start_time=start_time,
                    end_time=end_time,
                    frequency=frequency
                )
            )
        
        results = await asyncio.gather(*tasks, return_exceptions=True)
        
        # 整理结果
        portfolio_nav_data = {}
        for i, portfolio_id in enumerate(portfolio_ids):
            result = results[i]
            if isinstance(result, Exception):
                portfolio_nav_data[portfolio_id] = {
                    "success": False,
                    "error": str(result)
                }
            elif result is None:
                portfolio_nav_data[portfolio_id] = {
                    "success": False,
                    "error": "数据不存在"
                }
            else:
                nav_list = result.get('nav_list', [])
                portfolio_nav_data[portfolio_id] = {
                    "success": True,
                    "nav_data": nav_list,
                    "data_points": len(nav_list)
                }
        
        return {
            "success": True,
            "time_range": {
                "start_time": start_time,
                "end_time": end_time,
                "frequency": frequency
            },
            "portfolio_count": len(portfolio_ids),
            "results": portfolio_nav_data
        }
        
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"批量获取历史净资产失败: {str(e)}")
