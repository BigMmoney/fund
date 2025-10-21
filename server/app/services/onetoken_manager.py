#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
OneToken API 管理器
实现完整的投资组合管理功能
"""

import time
from typing import Dict, Any, List, Optional, Tuple
from server.app.api_clients import OneTokenClient
import logging

logger = logging.getLogger(__name__)

class OneTokenManager:
    """OneToken API管理器"""
    
    def __init__(self, api_key: str, api_secret: str, base_url: str):
        self.client = OneTokenClient(api_key, api_secret, base_url)
        self._portfolio_cache = {}
        self._account_cache = {}
    
    async def get_all_portfolios_with_accounts(self) -> Dict[str, Any]:
        """
        获取所有投资组合及其下属账户关系和投组本位币种
        
        实现步骤:
        1. 先拿到所有投组list (fundv3/openapi/portfolio/list-portfolio)
        2. 再传入单投组，多次调用拿到投组下属交易账户list (fundv3/openapi/portfolio/get-portfolio-detail)
        """
        try:
            # 步骤1: 获取所有投资组合列表
            logger.info("获取所有投资组合列表...")
            portfolios_result = await self.client.list_all_portfolios()
            
            if not portfolios_result:
                return {"error": "获取投资组合列表失败", "portfolios": []}
            
            portfolios = portfolios_result.get("fund_info_list", [])
            enhanced_portfolios = []
            
            # 步骤2: 为每个投资组合获取详细信息和下属账户
            for portfolio in portfolios:
                portfolio_id = portfolio.get("fund_name", "")
                portfolio_alias = portfolio.get("fund_alias", "")
                
                logger.info(f"获取投资组合详情: {portfolio_alias} ({portfolio_id})")
                
                # 获取投资组合详细信息
                portfolio_detail = await self.client.get_portfolio_detail(portfolio_id)
                
                # 整合数据
                enhanced_portfolio = {
                    # 基本信息
                    "id": portfolio_id,
                    "alias": portfolio_alias,
                    "display_name": self._get_portfolio_display_name(portfolio_alias),
                    
                    # 货币信息
                    "denomination": portfolio.get("denomination", "").upper(),
                    "valuation_currency": portfolio.get("valuation_currency", "").upper(),
                    
                    # 状态和配置
                    "status": portfolio.get("status", ""),
                    "status_display": self._get_status_display(portfolio.get("status", "")),
                    "auto_trading": portfolio.get("auto_ta_mode", False),
                    "version": portfolio.get("version", ""),
                    
                    # 时间信息
                    "inception_time": portfolio.get("inception_time_str", ""),
                    "creation_time": portfolio.get("creation_time_str", ""),
                    "last_operation": portfolio.get("operation_time_str", ""),
                    
                    # 运营信息
                    "operator": portfolio.get("operator", ""),
                    "strategy_type": self._get_strategy_type(portfolio_alias),
                    
                    # 下属账户信息
                    "accounts": self._extract_accounts_info(portfolio_detail),
                    "account_count": 0,
                    
                    # 原始数据
                    "raw_portfolio_data": portfolio,
                    "raw_detail_data": portfolio_detail
                }
                
                # 计算账户数量
                enhanced_portfolio["account_count"] = len(enhanced_portfolio["accounts"])
                
                enhanced_portfolios.append(enhanced_portfolio)
            
            # 整理返回结果
            result = {
                "success": True,
                "timestamp": time.time(),
                "total_portfolios": len(enhanced_portfolios),
                "portfolios": enhanced_portfolios,
                "summary": self._generate_portfolio_summary(enhanced_portfolios)
            }
            
            logger.info(f"成功获取 {len(enhanced_portfolios)} 个投资组合的完整信息")
            return result
            
        except Exception as e:
            logger.error(f"获取投资组合和账户关系失败: {e}")
            return {
                "success": False,
                "error": str(e),
                "portfolios": []
            }
    
    async def get_portfolio_hourly_nav_and_pnl(
        self, 
        portfolio_id: str, 
        start_time: Optional[int] = None, 
        end_time: Optional[int] = None,
        calculate_periods: List[str] = ["24h", "7d", "30d"]
    ) -> Dict[str, Any]:
        """
        获取hourly投组净资产，和基于累计收益相减计算指定时间区间/24H投组pnl
        
        使用API: fundv3/openapi/portfolio/get-historical-nav
        """
        try:
            # 设置默认时间范围 (最近30天，确保有足够数据)
            if start_time is None or end_time is None:
                end_time = int(time.time() * 1000000000)  # 纳秒时间戳
                start_time = end_time - (30 * 24 * 60 * 60 * 1000000000)  # 30天前
            
            logger.info(f"获取投资组合 {portfolio_id} 的历史净值数据...")
            
            # 获取hourly净值数据
            nav_result = await self.client.get_historical_nav(
                portfolio_id, start_time, end_time, "hourly"
            )
            
            if not nav_result:
                return {"error": "获取净值数据失败", "portfolio_id": portfolio_id}
            
            # 提取净值历史数据
            nav_history = nav_result.get("nav_history", [])
            if not nav_history:
                return {"error": "净值历史数据为空", "portfolio_id": portfolio_id}
            
            # 按时间排序
            nav_history.sort(key=lambda x: x.get("timestamp", 0))
            
            # 计算各个时间周期的PnL
            pnl_calculations = {}
            for period in calculate_periods:
                pnl_data = self._calculate_period_pnl(nav_history, period)
                pnl_calculations[period] = pnl_data
            
            # 生成结果
            result = {
                "success": True,
                "portfolio_id": portfolio_id,
                "currency": nav_result.get("currency", "USD"),
                "data_range": {
                    "start_time": start_time,
                    "end_time": end_time,
                    "total_hours": len(nav_history)
                },
                
                # 净值数据
                "nav_data": {
                    "latest_nav": nav_history[-1] if nav_history else None,
                    "first_nav": nav_history[0] if nav_history else None,
                    "hourly_history": nav_history,
                    "statistics": self._calculate_nav_statistics(nav_history)
                },
                
                # PnL计算
                "pnl_analysis": pnl_calculations,
                
                # 原始数据
                "raw_nav_result": nav_result
            }
            
            logger.info(f"成功获取投资组合 {portfolio_id} 的净值和PnL数据")
            return result
            
        except Exception as e:
            logger.error(f"获取投资组合净值和PnL失败: {e}")
            return {
                "success": False,
                "error": str(e),
                "portfolio_id": portfolio_id
            }
    
    def _extract_accounts_info(self, portfolio_detail: Optional[Dict]) -> List[Dict]:
        """从投资组合详情中提取账户信息"""
        if not portfolio_detail:
            return []
        
        accounts = []
        # 这里需要根据实际API返回结构调整
        account_list = portfolio_detail.get("accounts", [])
        
        for account in account_list:
            account_info = {
                "account_id": account.get("account_id", ""),
                "account_name": account.get("account_name", ""),
                "account_type": account.get("account_type", ""),
                "exchange": account.get("exchange", ""),
                "status": account.get("status", ""),
                "balance": account.get("balance", {}),
                "created_time": account.get("created_time", "")
            }
            accounts.append(account_info)
        
        return accounts
    
    def _calculate_period_pnl(self, nav_history: List[Dict], period: str) -> Dict[str, Any]:
        """计算指定时间周期的PnL"""
        if len(nav_history) < 2:
            return {
                "period": period,
                "error": "数据点不足",
                "data_points": len(nav_history)
            }
        
        # 计算时间窗口
        current_time = nav_history[-1].get("timestamp", 0)
        
        if period == "24h":
            window_start = current_time - (24 * 60 * 60 * 1000000000)
        elif period == "7d":
            window_start = current_time - (7 * 24 * 60 * 60 * 1000000000)
        elif period == "30d":
            window_start = current_time - (30 * 24 * 60 * 60 * 1000000000)
        else:
            return {"period": period, "error": "不支持的时间周期"}
        
        # 找到时间窗口内的数据
        period_data = [
            nav for nav in nav_history 
            if nav.get("timestamp", 0) >= window_start
        ]
        
        if len(period_data) < 2:
            return {
                "period": period,
                "error": f"时间窗口内数据点不足 ({len(period_data)})",
                "data_points": len(period_data)
            }
        
        # 计算PnL
        start_nav = period_data[0].get("nav", 0)
        end_nav = period_data[-1].get("nav", 0)
        
        absolute_pnl = end_nav - start_nav
        percentage_pnl = (absolute_pnl / start_nav * 100) if start_nav != 0 else 0
        
        return {
            "period": period,
            "start_time": period_data[0].get("timestamp"),
            "end_time": period_data[-1].get("timestamp"),
            "start_nav": start_nav,
            "end_nav": end_nav,
            "absolute_pnl": absolute_pnl,
            "percentage_pnl": round(percentage_pnl, 4),
            "data_points": len(period_data),
            "max_nav": max(nav.get("nav", 0) for nav in period_data),
            "min_nav": min(nav.get("nav", 0) for nav in period_data)
        }
    
    def _calculate_nav_statistics(self, nav_history: List[Dict]) -> Dict[str, Any]:
        """计算净值统计信息"""
        if not nav_history:
            return {}
        
        nav_values = [nav.get("nav", 0) for nav in nav_history]
        
        return {
            "count": len(nav_values),
            "max_nav": max(nav_values),
            "min_nav": min(nav_values),
            "avg_nav": sum(nav_values) / len(nav_values),
            "latest_nav": nav_values[-1] if nav_values else 0,
            "first_nav": nav_values[0] if nav_values else 0,
            "total_return": nav_values[-1] - nav_values[0] if len(nav_values) >= 2 else 0
        }
    
    def _get_portfolio_display_name(self, alias: str) -> str:
        """获取投资组合友好显示名称"""
        name_mapping = {
            "fund-demo-mixed": "混合策略基金",
            "fund-demo-defi": "DeFi策略基金",
            "fund-dmeo-cefi": "CeFi策略基金",
            "fund-demo-cefi": "CeFi策略基金"
        }
        return name_mapping.get(alias, alias)
    
    def _get_status_display(self, status: str) -> str:
        """获取状态显示名称"""
        status_mapping = {
            "running": "运行中",
            "stopped": "已停止",
            "paused": "已暂停",
            "liquidating": "清算中"
        }
        return status_mapping.get(status, status)
    
    def _get_strategy_type(self, alias: str) -> str:
        """获取策略类型"""
        alias_lower = alias.lower()
        if "mixed" in alias_lower:
            return "mixed"
        elif "defi" in alias_lower:
            return "defi"
        elif "cefi" in alias_lower:
            return "cefi"
        else:
            return "other"
    
    def _generate_portfolio_summary(self, portfolios: List[Dict]) -> Dict[str, Any]:
        """生成投资组合汇总信息"""
        if not portfolios:
            return {}
        
        # 统计状态分布
        status_count = {}
        currency_count = {}
        strategy_count = {}
        auto_trading_count = 0
        total_accounts = 0
        
        for portfolio in portfolios:
            # 状态统计
            status = portfolio.get("status", "unknown")
            status_count[status] = status_count.get(status, 0) + 1
            
            # 货币统计
            currency = portfolio.get("denomination", "unknown")
            currency_count[currency] = currency_count.get(currency, 0) + 1
            
            # 策略统计
            strategy = portfolio.get("strategy_type", "unknown")
            strategy_count[strategy] = strategy_count.get(strategy, 0) + 1
            
            # 自动交易统计
            if portfolio.get("auto_trading", False):
                auto_trading_count += 1
            
            # 账户统计
            total_accounts += portfolio.get("account_count", 0)
        
        return {
            "total_portfolios": len(portfolios),
            "total_accounts": total_accounts,
            "auto_trading_portfolios": auto_trading_count,
            "status_distribution": status_count,
            "currency_distribution": currency_count,
            "strategy_distribution": strategy_count,
            "avg_accounts_per_portfolio": round(total_accounts / len(portfolios), 2)
        }

    async def batch_get_portfolios_nav_and_pnl(self, portfolio_ids: List[str]) -> Dict[str, Any]:
        """批量获取多个投资组合的净值和PnL数据"""
        results = {}
        
        for portfolio_id in portfolio_ids:
            logger.info(f"处理投资组合: {portfolio_id}")
            result = await self.get_portfolio_hourly_nav_and_pnl(portfolio_id)
            results[portfolio_id] = result
        
        return {
            "success": True,
            "total_processed": len(portfolio_ids),
            "results": results,
            "timestamp": time.time()
        }
