import httpx
import base64
import hashlib
import hmac
import json
import time
from typing import Dict, Any, Optional, List
from server.app.settings import settings

class CeffuClient:
    def __init__(self, api_key: str, api_secret: str, base_url: str):
        self.api_key = api_key
        self.api_secret = api_secret
        self.base_url = base_url

    async def fetch_data(self) -> Dict[str, Any]:
        # 示例：获取账户信息
        async with httpx.AsyncClient() as client:
            # 这里需要根据 ceffu API 文档补充签名和参数
            resp = await client.get(f"{self.base_url}/v1/account", headers={"X-API-KEY": self.api_key})
            return resp.json()

class OneTokenClient:
    def __init__(self, api_key: str = None, api_secret: str = None, base_url: str = None):
        # 使用传入的参数或配置文件中的默认值
        self.api_key = api_key or settings.onetoken_api_key
        self.api_secret = api_secret or settings.onetoken_api_secret
        self.base_url = base_url or settings.onetoken_base_url
        
        # 根据base_url调整url_prefix
        if "fundv3.1token.trade" in self.base_url:
            # 新的OneToken Fund API
            self.url_prefix = self.base_url
        else:
            # 原始API格式
            self.url_prefix = self.base_url + "/api/v1"

    def _gen_timestamp(self) -> int:
        """生成时间戳"""
        return int(time.time())

    def _gen_sign(self, secret: str, verb: str, path: str, timestamp: int, data: Optional[Dict] = None) -> str:
        """生成签名"""
        if data is None:
            data_str = ""
        else:
            data_str = json.dumps(data)
        
        message = verb + path + str(timestamp) + data_str
        signature = hmac.new(
            base64.b64decode(secret), 
            bytes(message, "utf8"), 
            digestmod=hashlib.sha256
        )
        return base64.b64encode(signature.digest()).decode()

    def _gen_headers(self, timestamp: int, signature: str) -> Dict[str, str]:
        """生成请求头"""
        return {
            "Api-Timestamp": str(timestamp),
            "Api-Key": self.api_key,
            "Api-Signature": signature,
            "Content-Type": "application/json",
        }

    async def _api_request(self, method: str, path: str, data: Optional[Dict] = None) -> Optional[Dict]:
        """通用API请求方法"""
        timestamp = self._gen_timestamp()
        signature = self._gen_sign(self.api_secret, method, path, timestamp, data)
        headers = self._gen_headers(timestamp, signature)
        
        async with httpx.AsyncClient(timeout=30) as client:
            try:
                if data is not None:
                    post_data = json.dumps(data)
                    resp = await client.request(
                        method, 
                        self.url_prefix + path, 
                        headers=headers, 
                        content=post_data
                    )
                else:
                    resp = await client.request(method, self.url_prefix + path, headers=headers)
                
                return resp.json()
            except Exception as e:
                print(f"API请求错误: {e}")
                return None

    async def ping(self) -> Optional[Dict]:
        """测试连接"""
        return await self._api_request("GET", "/httpmisc/ping")

    async def list_all_portfolios(self) -> Optional[Dict]:
        """列出所有投资组合"""
        resp = await self._api_request("GET", "/fundv3/openapi/portfolio/list-portfolio")
        return resp if resp else None

    async def get_portfolio_detail(self, portfolio_id: str) -> Optional[Dict]:
        """获取投资组合详情，包括下属交易账户列表"""
        data = {"portfolio_id": portfolio_id}
        resp = await self._api_request("POST", "/fundv3/openapi/portfolio/get-portfolio-detail", data)
        return resp if resp else None

    async def get_historical_nav(
        self, 
        portfolio_id: str, 
        start_time: int, 
        end_time: int, 
        frequency: str = "hourly"
    ) -> Optional[Dict]:
        """获取投资组合历史净资产"""
        data = {
            "portfolio_id": portfolio_id,
            "start_time": start_time,
            "end_time": end_time,
            "frequency": frequency
        }
        resp = await self._api_request("POST", "/fundv3/openapi/portfolio/get-historical-nav", data)
        return resp if resp else None

    async def list_all_accounts(self) -> Optional[Dict]:
        """列出所有交易账户"""
        resp = await self._api_request("GET", "/tradeacc/list-all-accounts")
        return resp.get("account", {}) if resp else None

    async def get_fund_asset_position(
        self, 
        funds: List[str], 
        start_time: int, 
        end_time: int, 
        frequency: str = "daily",
        quote_source: str = "cmc_close",
        equity_valuation_currency: str = "usd",
        offset: str = "8h"
    ) -> Optional[Dict]:
        """获取基金资产持仓历史快照"""
        data = {
            "fund_list": funds,
            "start_time": start_time,
            "end_time": end_time,
            "frequency": frequency,
            "quote_source": quote_source,
            "equity_valuation_currency": equity_valuation_currency,
            "offset": offset,
        }
        resp = await self._api_request("POST", "/anp/openapi/fund-snapshot/get-asset-position", data)
        return resp.get("result", {}) if resp else None

    async def get_account_asset_position(
        self,
        accounts: List[str],
        start_time: int,
        end_time: int,
        frequency: str = "daily",
        quote_source: str = "cmc_close",
        equity_valuation_currency: str = "usd",
        offset: str = "8h"
    ) -> Optional[Dict]:
        """获取账户资产持仓历史快照"""
        data = {
            "account_list": accounts,
            "start_time": start_time,
            "end_time": end_time,
            "frequency": frequency,
            "quote_source": quote_source,
            "equity_valuation_currency": equity_valuation_currency,
            "offset": offset,
        }
        resp = await self._api_request("POST", "/anp/openapi/account-snapshot/get-asset-position", data)
        return resp.get("result", {}) if resp else None

    async def get_fund_coin_exposure_data(self, funds: List[str]) -> Optional[Dict]:
        """获取投资组合币种敞口数据"""
        data = {"fund_list": funds}
        resp = await self._api_request("POST", "/anp/openapi/fund/get-coin-exposure-data", data)
        return resp.get("result", {}) if resp else None

    async def get_portfolio_detail(self, portfolio_id: str) -> Optional[Dict]:
        """获取投资组合详情，包括下属交易账户列表"""
        data = {"portfolio_id": portfolio_id}
        resp = await self._api_request("POST", "/fundv3/openapi/portfolio/get-portfolio-detail", data)
        return resp.get("result", {}) if resp else None

    async def get_historical_nav(
        self, 
        portfolio_id: str, 
        start_time: int, 
        end_time: int, 
        frequency: str = "hourly"
    ) -> Optional[Dict]:
        """获取投资组合历史净资产"""
        data = {
            "portfolio_id": portfolio_id,
            "start_time": start_time,
            "end_time": end_time,
            "frequency": frequency
        }
        resp = await self._api_request("POST", "/fundv3/openapi/portfolio/get-historical-nav", data)
        return resp.get("result", {}) if resp else None

    async def fetch_data(self) -> Dict[str, Any]:
        """获取基本数据 - 兼容原接口"""
        ping_result = await self.ping()
        portfolios = await self.list_all_portfolios()
        accounts = await self.list_all_accounts()
        
        return {
            "ping": ping_result,
            "portfolios": portfolios,
            "accounts": accounts
        }
