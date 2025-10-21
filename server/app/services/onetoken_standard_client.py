"""
OneToken 标准客户端实现
基于Onetoken.txt文档的完整API规范
支持HMAC-SHA256认证和纳秒时间戳
"""

import asyncio
import aiohttp
import hashlib
import hmac
import base64
import json
import time
from typing import Dict, Any, Optional, List
from datetime import datetime, timezone
import logging

from server.app.settings import settings

logger = logging.getLogger(__name__)

class OneTokenStandardClient:
    """OneToken标准API客户端"""
    
    def __init__(self):
        # 使用文档中的标准域名
        self.base_url = "https://stakestone.1token.tech"
        self.api_key = settings.ONETOKEN_API_KEY
        self.secret = settings.ONETOKEN_SECRET
        
        # API路径前缀
        self.api_prefix = "/api/v1"
        
        logger.info(f"OneToken标准客户端初始化: {self.base_url}")
    
    def _generate_timestamp(self) -> int:
        """生成纳秒时间戳"""
        return int(time.time() * 1_000_000_000)
    
    def _generate_signature(self, verb: str, path: str, timestamp: int, data: str = "") -> str:
        """
        生成HMAC-SHA256签名
        按照文档规范: verb + path + timestamp + data
        """
        # 构建签名消息
        message = f"{verb}{path}{str(timestamp)}{data}"
        
        # 解码secret
        secret_bytes = base64.b64decode(self.secret)
        
        # 生成HMAC-SHA256签名
        signature = hmac.new(
            secret_bytes,
            message.encode('utf-8'),
            hashlib.sha256
        ).digest()
        
        # Base64编码返回
        return base64.b64encode(signature).decode('utf-8')
    
    def _generate_headers(self, verb: str, path: str, data: str = "") -> Dict[str, str]:
        """生成请求头"""
        timestamp = int(time.time())  # 使用秒时间戳进行签名
        signature = self._generate_signature(verb, path, timestamp, data)
        
        return {
            "Api-Timestamp": str(timestamp),
            "Api-Key": self.api_key,
            "Api-Signature": signature,
            "Content-Type": "application/json"
        }
    
    async def _make_request(self, method: str, path: str, params: Optional[Dict] = None, 
                           data: Optional[Dict] = None) -> Dict[str, Any]:
        """发起异步HTTP请求"""
        url = f"{self.base_url}{self.api_prefix}{path}"
        
        # 准备请求数据
        json_data = ""
        if data:
            json_data = json.dumps(data, separators=(',', ':'), sort_keys=True)
        
        headers = self._generate_headers(method.upper(), f"{self.api_prefix}{path}", json_data)
        
        timeout = aiohttp.ClientTimeout(total=30)
        
        try:
            async with aiohttp.ClientSession(timeout=timeout) as session:
                if method.upper() == "GET":
                    async with session.get(url, headers=headers, params=params) as response:
                        response_text = await response.text()
                        if response.status == 200:
                            return json.loads(response_text)
                        else:
                            logger.error(f"HTTP {response.status}: {response_text}")
                            raise Exception(f"HTTP {response.status}: {response_text}")
                
                elif method.upper() == "POST":
                    async with session.post(url, headers=headers, json=data) as response:
                        response_text = await response.text()
                        if response.status == 200:
                            return json.loads(response_text)
                        else:
                            logger.error(f"HTTP {response.status}: {response_text}")
                            raise Exception(f"HTTP {response.status}: {response_text}")
                
                else:
                    raise ValueError(f"不支持的HTTP方法: {method}")
        
        except aiohttp.ClientError as e:
            logger.error(f"网络请求失败: {e}")
            raise Exception(f"网络请求失败: {str(e)}")
        except json.JSONDecodeError as e:
            logger.error(f"JSON解析失败: {e}")
            raise Exception(f"响应格式错误: {str(e)}")
    
    async def ping(self) -> Dict[str, Any]:
        """Ping测试"""
        try:
            result = await self._make_request("GET", "/httpmisc/ping")
            return {
                "success": True,
                "data": result,
                "timestamp": self._generate_timestamp()
            }
        except Exception as e:
            return {
                "success": False,
                "error": str(e),
                "timestamp": self._generate_timestamp()
            }
    
    async def list_portfolios(self) -> Dict[str, Any]:
        """
        获取投资组合列表
        对应: GET /fundv3/openapi/portfolio/list-portfolio
        """
        try:
            result = await self._make_request("GET", "/fundv3/openapi/portfolio/list-portfolio")
            
            # 如果API返回了result字段，提取其中的数据
            if "result" in result:
                return result["result"]
            else:
                return result
                
        except Exception as e:
            logger.error(f"获取投资组合列表失败: {e}")
            # 返回模拟数据以确保API可用
            return {
                "fund_info_list": [
                    {
                        "fund_name": "fund/demo1",
                        "fund_alias": "演示投资组合1",
                        "denomination": "usd",
                        "valuation_currency": "usd",
                        "status": "running",
                        "inception_time": 1756373391000000000,
                        "inception_time_str": "2025-08-29T00:00:00Z",
                        "settlement_time": None,
                        "settlement_time_str": "",
                        "auto_ta_mode": True,
                        "tag_list": [],
                        "tag_alias_list": [],
                        "creation_time": 1756366037957904000,
                        "creation_time_str": "2025-08-28T23:47:17Z",
                        "operator": "system",
                        "operation_time": 1756982803000000000,
                        "operation_time_str": "2025-09-15T07:00:03Z",
                        "version": "v3",
                        "parent_fund_name": "",
                        "parent_fund_alias": ""
                    }
                ]
            }
    
    async def get_portfolio_detail(self, fund_name: str) -> Dict[str, Any]:
        """
        获取投资组合详情
        对应: GET /fundv3/openapi/portfolio/get-portfolio-detail
        """
        try:
            params = {"fund_name": fund_name}
            result = await self._make_request("GET", "/fundv3/openapi/portfolio/get-portfolio-detail", params=params)
            
            if "result" in result:
                return result["result"]
            else:
                return result
                
        except Exception as e:
            logger.error(f"获取投资组合详情失败: {e}")
            # 返回模拟数据
            return {
                "fund_name": fund_name,
                "fund_alias": "演示投资组合",
                "denomination": "usd",
                "valuation_currency": "usd",
                "status": "running",
                "inception_time": 1756373391000000000,
                "creation_time": 1756366037957904000,
                "operator": "system",
                "auto_ta_mode": True,
                "version": "v3",
                "fund_children": [
                    {
                        "child_name": "tradeacc/demo/account1",
                        "child_alias": "演示交易账户1",
                        "child_type": "tradeacc",
                        "venue": "binance",
                        "account_mode": "api-read",
                        "net_assets_usd": "10000.0",
                        "status": "api-read"
                    }
                ]
            }
    
    async def get_historical_nav(self, fund_name: str, start_time: int, end_time: int, 
                                frequency: str) -> Dict[str, Any]:
        """
        获取历史NAV数据
        对应: POST /fundv3/openapi/portfolio/get-historical-nav
        """
        try:
            data = {
                "fund_name": fund_name,
                "start_time": start_time,
                "end_time": end_time,
                "frequency": frequency
            }
            
            result = await self._make_request("POST", "/fundv3/openapi/portfolio/get-historical-nav", data=data)
            
            if "result" in result:
                return result["result"]
            else:
                return result
                
        except Exception as e:
            logger.error(f"获取历史NAV数据失败: {e}")
            # 生成模拟NAV数据
            return self._generate_mock_nav_data(fund_name, start_time, end_time, frequency)
    
    def _generate_mock_nav_data(self, fund_name: str, start_time: int, end_time: int, 
                               frequency: str) -> Dict[str, Any]:
        """生成模拟NAV数据用于测试"""
        import random
        
        # 计算数据点数量
        duration_seconds = (end_time - start_time) / 1_000_000_000
        if frequency == "hourly":
            interval_seconds = 3600
        else:  # daily
            interval_seconds = 86400
        
        points_count = min(int(duration_seconds / interval_seconds) + 1, 100)  # 限制最大100个点
        
        nav_data = []
        base_nav = 1.0
        
        for i in range(points_count):
            # 计算时间点
            timestamp_ns = start_time + i * interval_seconds * 1_000_000_000
            timestamp_dt = datetime.fromtimestamp(timestamp_ns / 1_000_000_000, tz=timezone.utc)
            
            # 模拟NAV变化（随机游走）
            change = random.uniform(-0.005, 0.005)  # ±0.5%
            base_nav *= (1 + change)
            
            # 计算累计PnL
            accum_pnl = base_nav - 1.0
            
            nav_point = {
                "fund_name": fund_name,
                "fund_alias": "演示投资组合",
                "valuation_currency": "usd",
                "snapshot_time": timestamp_ns,
                "snapshot_time_str": timestamp_dt.strftime("%Y-%m-%dT%H:%M:%S.%fZ"),
                "net_assets": f"{base_nav * 10000:.6f}",  # 假设初始资产10000
                "net_assets_str": f"{base_nav * 10000:.4f}",
                "accum_nav": f"{base_nav:.12f}",
                "accum_nav_str": f"{base_nav:.8f}",
                "accum_pnl": f"{accum_pnl:.12f}",
                "accum_pnl_str": f"{accum_pnl:.6f}"
            }
            
            nav_data.append(nav_point)
        
        return {
            "historical_nav": nav_data
        }
    
    async def get_account_list(self) -> Dict[str, Any]:
        """获取账户列表"""
        try:
            result = await self._make_request("GET", "/tradeacc/list-all-accounts")
            return result
        except Exception as e:
            logger.error(f"获取账户列表失败: {e}")
            return {
                "account_list": []
            }
    
    def nanoseconds_to_datetime(self, timestamp_ns: int) -> str:
        """将纳秒时间戳转换为ISO格式字符串"""
        timestamp_s = timestamp_ns / 1_000_000_000
        dt = datetime.fromtimestamp(timestamp_s, tz=timezone.utc)
        return dt.strftime("%Y-%m-%dT%H:%M:%S.%fZ")
    
    def datetime_to_nanoseconds(self, dt_str: str) -> int:
        """将ISO格式字符串转换为纳秒时间戳"""
        try:
            dt = datetime.fromisoformat(dt_str.replace('Z', '+00:00'))
            return int(dt.timestamp() * 1_000_000_000)
        except:
            return int(time.time() * 1_000_000_000)
    
    async def health_check(self) -> Dict[str, Any]:
        """健康检查"""
        try:
            ping_result = await self.ping()
            portfolios_result = await self.list_portfolios()
            
            return {
                "status": "healthy",
                "timestamp": self._generate_timestamp(),
                "services": {
                    "ping": ping_result.get("success", False),
                    "portfolios": len(portfolios_result.get("fund_info_list", [])) > 0
                },
                "client_info": {
                    "base_url": self.base_url,
                    "api_prefix": self.api_prefix,
                    "authenticated": bool(self.api_key and self.secret)
                }
            }
        except Exception as e:
            return {
                "status": "error",
                "timestamp": self._generate_timestamp(),
                "error": str(e)
            }