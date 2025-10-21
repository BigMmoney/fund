"""
OneToken API客户端
基于官方文档实现的真实API集成，使用requests库
"""
import hashlib
import hmac
import base64
import time
import json
from typing import Dict, List, Optional, Any
from datetime import datetime
import requests
from server.app.settings import settings

class OneTokenClient:
    """OneToken API客户端"""
    
    def __init__(self):
        self.base_url = settings.ONETOKEN_BASE_URL
        self.api_key = settings.ONETOKEN_API_KEY
        self.secret = settings.ONETOKEN_SECRET
        
    def _create_signature(self, verb: str, path: str, timestamp: str, data: str = "") -> str:
        """
        创建签名 - 关键修复：path 不能包含 /api/v1 前缀
        
        根据 1tokenn_apiNEW.txt 官方示例：
        - URL 构建: url_prefix ("/api/v1") + path
        - 签名构建: verb + path + timestamp + data (path 不含 /api/v1)
        """
        # 移除 path 中的 /api/v1 前缀（如果存在）
        if path.startswith("/api/v1"):
            path = path[7:]  # 移除 "/api/v1"
        
        # 构建签名字符串: verb + path + timestamp + data
        message = f"{verb}{path}{timestamp}{data}"
        
        # base64解码secret
        secret_bytes = base64.b64decode(self.secret)
        
        # 创建HMAC-SHA256签名
        signature = hmac.new(
            secret_bytes,
            message.encode('utf-8'),
            hashlib.sha256
        ).digest()
        
        # base64编码签名
        return base64.b64encode(signature).decode('utf-8')
    
    def _make_request(self, method: str, path: str, params: Optional[Dict] = None, 
                      data: Optional[Dict] = None) -> Dict[str, Any]:
        """
        发起API请求 - 使用正确的认证头格式
        
        根据测试成功的方法：
        - 请求头使用: Api-Key, Api-Signature, Api-Timestamp
        - 时间戳使用整数秒
        - JSON 序列化不指定 separators（使用默认格式）
        """
        # 生成秒时间戳
        timestamp = int(time.time())
        
        # 准备请求数据 - 按照官方示例，不指定 separators
        request_data = ""
        if data:
            request_data = json.dumps(data)  # 使用默认格式
        
        # 创建签名（path 会在函数内部移除 /api/v1）
        signature = self._create_signature(method, path, str(timestamp), request_data)
        
        # 设置请求头 - 使用测试成功的格式
        headers = {
            "Api-Key": self.api_key,
            "Api-Signature": signature,
            "Api-Timestamp": str(timestamp),
            "Content-Type": "application/json"
        }
        
        # 构建完整URL
        url = f"{self.base_url}{path}"
        
        # 发起请求
        try:
            if method.upper() == "GET":
                response = requests.get(url, headers=headers, params=params, timeout=30)
            elif method.upper() == "POST":
                # POST 请求直接传递 JSON 字符串作为 data
                response = requests.post(
                    url, 
                    headers=headers, 
                    data=request_data.encode('utf-8') if request_data else None,
                    timeout=30
                )
            else:
                raise ValueError(f"不支持的HTTP方法: {method}")
            
            # 检查响应状态
            response.raise_for_status()
            
            return response.json()
            
        except requests.exceptions.RequestException as e:
            # 返回错误信息，而不是抛出异常
            return {
                "error": str(e),
                "status_code": getattr(e.response, 'status_code', None) if hasattr(e, 'response') else None,
                "response_text": getattr(e.response, 'text', '') if hasattr(e, 'response') else ''
            }
    
    def get_portfolios(self) -> Dict[str, Any]:
        """
        获取所有投组列表
        使用测试成功的端点: GET /api/v1/fundv3/openapi/portfolio/list-portfolio
        """
        return self._make_request("GET", "/api/v1/fundv3/openapi/portfolio/list-portfolio")
    
    def get_portfolio_detail(self, fund_name: str) -> Dict[str, Any]:
        """
        获取投资组合详情（包含子账户）
        使用测试成功的端点: GET /api/v1/fundv3/openapi/portfolio/get-portfolio-detail
        """
        path = f"/api/v1/fundv3/openapi/portfolio/get-portfolio-detail?fund_name={fund_name}"
        return self._make_request("GET", path)
    
    def get_all_accounts(self) -> Dict[str, Any]:
        """获取所有账户及关系"""  
        return self._make_request("GET", "/api/v1/tradeacc/list-all-accounts")
    
    def get_portfolio_historical_nav(self, fund_name: str, start_time: int, 
                                     end_time: int, frequency: str = "hourly") -> Dict[str, Any]:
        """
        获取投组历史净值数据
        使用测试成功的端点: POST /api/v1/fundv3/openapi/portfolio/get-historical-nav
        
        Args:
            fund_name: 投资组合名称，如 "fund/cendmz"
            start_time: 开始时间（纳秒时间戳）
            end_time: 结束时间（纳秒时间戳）
            frequency: 数据频率，"hourly" 或 "daily"
        """
        data = {
            "fund_name": fund_name,
            "start_time": start_time,
            "end_time": end_time,
            "frequency": frequency
        }
        return self._make_request("POST", "/api/v1/fundv3/openapi/portfolio/get-historical-nav", data=data)
    
    def get_portfolio_nav_history(self, portfolio_name: str, start_time: str, 
                                      end_time: str) -> Dict[str, Any]:
        """
        获取投组净值历史数据（旧接口兼容）
        内部调用 get_portfolio_historical_nav
        """
        # 转换时间戳格式（假设输入是纳秒）
        start_ns = int(start_time) if isinstance(start_time, str) else start_time
        end_ns = int(end_time) if isinstance(end_time, str) else end_time
        
        return self.get_portfolio_historical_nav(portfolio_name, start_ns, end_ns, "hourly")
    
    def get_account_nav_history(self, account_symbol: str, start_time: str, 
                                    end_time: str) -> Dict[str, Any]:
        """获取账户净值历史数据"""
        params = {
            "account": account_symbol,
            "start_time": start_time,
            "end_time": end_time
        }
        return self._make_request("GET", "/api/v1/tradeacc/get-acc-nav-history", params=params)
    
    def process_portfolio_data(self, portfolio_data: Dict[str, Any]) -> Dict[str, Any]:
        """
        处理投组数据，提取投组基本信息
        适配新的响应格式：result.fund_info_list
        """
        result = {
            "portfolios": [],
            "summary": {}
        }
        
        # 检查新格式：result.fund_info_list
        if "result" in portfolio_data and "fund_info_list" in portfolio_data["result"]:
            fund_list = portfolio_data["result"]["fund_info_list"]
        elif "data" in portfolio_data and "fund_info_list" in portfolio_data["data"]:
            # 兼容旧格式
            fund_list = portfolio_data["data"]["fund_info_list"]
        else:
            return result
        
        for fund in fund_list:
            portfolio_info = {
                "portfolio_name": fund.get("fund_name", ""),
                "portfolio_alias": fund.get("fund_alias", ""),
                "base_currency": fund.get("denomination", ""),  # 新格式字段
                "valuation_currency": fund.get("valuation_currency", ""),  # 新格式字段
                "status": fund.get("status", ""),
                "inception_time": fund.get("inception_time_str", ""),
                "creation_time": fund.get("creation_time_str", ""),
                "operator": fund.get("operator", ""),
                "auto_ta_mode": fund.get("auto_ta_mode", False),
                "version": fund.get("version", ""),
                "tag_list": fund.get("tag_list", []),
                "tag_alias_list": fund.get("tag_alias_list", [])
            }
            
            result["portfolios"].append(portfolio_info)
        
        result["summary"] = {
            "total_portfolios": len(fund_list),
            "request_time": portfolio_data.get("request_time_str", ""),
            "response_time": portfolio_data.get("response_time_str", "")
        }
        
        return result
        
    def process_account_data(self, accounts_data: Dict[str, Any]) -> Dict[str, Any]:
        result = {
            "portfolios": [],
            "accounts": [],
            "relationships": []
        }
        
        if "data" not in accounts_data:
            return result
        
        accounts = accounts_data["data"]
        
        for account in accounts:
            # 处理主账户
            account_info = {
                "account_id": account.get("id"),
                "name": account.get("name"),
                "alias": account.get("alias"),
                "exchange": account.get("exchange"),
                "exchange_str": account.get("exchange_str"),
                "account_type": account.get("account_type"),
                "account_type_str": account.get("account_type_str"),
                "asset_base": account.get("asset_base", ""),  # 基础货币
                "balance": account.get("balance", "0"),
                "balance_translate": account.get("balance_translate", "0"),
                "balance_usdt_translate": account.get("balance_usdt_translate", "0"),
                "is_root": account.get("is_root", False),
                "fund_name": account.get("fund_name", ""),
                "fund_alias": account.get("fund_alias", ""),
                "api_status": account.get("api_status"),
                "creation_time": account.get("creation_time_str")
            }
            
            result["accounts"].append(account_info)
            
            # 如果有投组信息，添加到投组列表
            if account.get("fund_name"):
                portfolio_info = {
                    "portfolio_name": account.get("fund_name"),
                    "portfolio_alias": account.get("fund_alias", ""),
                    "base_currency": account.get("asset_base", ""),
                    "accounts": []
                }
                
                # 检查是否已存在该投组
                existing_portfolio = next(
                    (p for p in result["portfolios"] if p["portfolio_name"] == portfolio_info["portfolio_name"]), 
                    None
                )
                
                if not existing_portfolio:
                    result["portfolios"].append(portfolio_info)
                    existing_portfolio = portfolio_info
                
                # 添加账户到投组
                existing_portfolio["accounts"].append(account_info["name"])
            
            # 处理子账户
            for child_account in account.get("child", []):
                child_info = {
                    "account_id": child_account.get("id"),
                    "name": child_account.get("name"),
                    "alias": child_account.get("alias"),
                    "exchange": child_account.get("exchange"),
                    "exchange_str": child_account.get("exchange_str"),
                    "account_type": child_account.get("account_type"),
                    "account_type_str": child_account.get("account_type_str"),
                    "asset_base": child_account.get("asset_base", ""),
                    "balance": child_account.get("balance"),
                    "balance_translate": child_account.get("balance_translate"),
                    "balance_usdt_translate": child_account.get("balance_usdt_translate"),
                    "is_root": False,
                    "parent_account": account.get("name"),
                    "fund_name": child_account.get("fund_name", ""),
                    "fund_alias": child_account.get("fund_alias", ""),
                    "api_status": child_account.get("api_status"),
                    "creation_time": child_account.get("creation_time_str")
                }
                
                result["accounts"].append(child_info)
                
                # 添加关系映射
                result["relationships"].append({
                    "parent_account": account.get("name"),
                    "child_account": child_account.get("name"),
                    "relationship_type": "parent_child"
                })
                
                # 如果子账户有投组信息，添加到对应投组
                if child_account.get("fund_name"):
                    existing_portfolio = next(
                        (p for p in result["portfolios"] 
                         if p["portfolio_name"] == child_account.get("fund_name")), 
                        None
                    )
                    
                    if existing_portfolio and child_info["name"] not in existing_portfolio["accounts"]:
                        existing_portfolio["accounts"].append(child_info["name"])
        
        return result
    
    def calculate_portfolio_pnl(self, portfolio_name: str, start_time: int, 
                                    end_time: int, frequency: str = "hourly") -> Dict[str, Any]:
        """
        计算投组指定时间区间的PnL
        使用新的 get_portfolio_historical_nav 接口
        
        Args:
            portfolio_name: 投资组合名称
            start_time: 开始时间（纳秒时间戳）
            end_time: 结束时间（纳秒时间戳）
            frequency: 数据频率，"hourly" 或 "daily"
        """
        try:
            # 获取投组历史数据
            hist_data = self.get_portfolio_historical_nav(
                portfolio_name, start_time, end_time, frequency
            )
            
            # 检查响应格式
            if "result" not in hist_data or "historical_nav" not in hist_data["result"]:
                return {"error": "无法获取投组历史数据", "raw_response": hist_data}
            
            nav_data = hist_data["result"]["historical_nav"]
            
            if len(nav_data) < 2:
                return {"error": "数据点不足，无法计算PnL", "data_points": len(nav_data)}
            
            # 获取起始和结束净资产（使用 net_assets 字段）
            start_net_assets = float(nav_data[0].get("net_assets", 0))
            end_net_assets = float(nav_data[-1].get("net_assets", 0))
            
            # 或者使用累计 PnL（accum_pnl）
            start_accum_pnl = float(nav_data[0].get("accum_pnl", 0))
            end_accum_pnl = float(nav_data[-1].get("accum_pnl", 0))
            
            # 计算区间 PnL
            pnl = end_accum_pnl - start_accum_pnl
            pnl_percentage = (pnl / start_net_assets * 100) if start_net_assets != 0 else 0
            
            # 获取货币信息
            currency = nav_data[0].get("valuation_currency", "")
            
            return {
                "portfolio": portfolio_name,
                "portfolio_alias": nav_data[0].get("fund_alias", ""),
                "start_time": nav_data[0].get("snapshot_time_str", ""),
                "end_time": nav_data[-1].get("snapshot_time_str", ""),
                "start_net_assets": start_net_assets,
                "end_net_assets": end_net_assets,
                "start_accum_pnl": start_accum_pnl,
                "end_accum_pnl": end_accum_pnl,
                "interval_pnl": pnl,
                "pnl_percentage": pnl_percentage,
                "currency": currency,
                "data_points": len(nav_data),
                "frequency": frequency,
                "hourly_data": [
                    {
                        "time": point.get("snapshot_time_str", ""),
                        "net_assets": float(point.get("net_assets", 0)),
                        "accum_nav": float(point.get("accum_nav", 0)),
                        "accum_pnl": float(point.get("accum_pnl", 0)),
                        "currency": point.get("valuation_currency", "")
                    }
                    for point in nav_data
                ]
            }
            
        except Exception as e:
            return {"error": f"计算投组PnL时出错: {str(e)}"}

    def get_24h_portfolio_pnl(self, portfolio_name: str) -> Dict[str, Any]:
        """获取24小时投组PnL"""
        # 计算24小时前的时间戳（纳秒）
        current_time = int(time.time() * 1000000000)
        start_time = current_time - (24 * 60 * 60 * 1000000000)  # 24小时前
        
        return self.calculate_portfolio_pnl(
            portfolio_name, 
            start_time, 
            current_time,
            "hourly"
        )

# 创建全局客户端实例
onetoken_client = OneTokenClient()
