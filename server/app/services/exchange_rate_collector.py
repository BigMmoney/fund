"""
汇率数据采集器
Exchange Rate Collector

负责采集汇率数据，包括:
- USD/CNY
- BTC/USD
- ETH/USD
- 其他主要币种的USD价格
"""

from typing import List, Dict, Any, Optional
from datetime import datetime
from decimal import Decimal
from sqlalchemy.orm import Session
import logging
import aiohttp

from server.app.services.data_collector import DataCollector, CollectionResult
from server.app.models.snapshots import RateSnapshot
from server.app.database import SessionLocal

logger = logging.getLogger(__name__)


class ExchangeRateCollector(DataCollector):
    """
    汇率数据采集器
    
    采集各种货币对的汇率数据并保存到数据库
    """
    
    # 需要采集的货币对
    CURRENCY_PAIRS = [
        ("USD", "CNY"),   # 美元/人民币
        ("USD", "EUR"),   # 美元/欧元
        ("USD", "JPY"),   # 美元/日元
    ]
    
    # 需要采集的加密货币价格 (相对USD)
    CRYPTO_SYMBOLS = [
        "BTC",   # Bitcoin
        "ETH",   # Ethereum
        "BNB",   # Binance Coin
        "USDT",  # Tether (应该是1.0)
        "USDC",  # USD Coin (应该是1.0)
    ]
    
    def __init__(self):
        super().__init__("ExchangeRateCollector")
        self.fiat_api_url = "https://api.exchangerate-api.com/v4/latest/USD"
        self.crypto_api_url = "https://api.coingecko.com/api/v3/simple/price"
    
    async def collect(self, timestamp: datetime) -> CollectionResult:
        """
        采集汇率数据
        
        Steps:
        1. 采集法币汇率 (USD/CNY, USD/EUR等)
        2. 采集加密货币价格 (BTC/USD, ETH/USD等)
        3. 合并数据
        
        Returns:
            CollectionResult: 包含所有汇率数据
        """
        try:
            self.logger.info(f"Collecting exchange rates at {timestamp}")
            
            collected_data = []
            
            # 采集法币汇率
            fiat_rates = await self._collect_fiat_rates(timestamp)
            collected_data.extend(fiat_rates)
            
            # 采集加密货币价格
            crypto_rates = await self._collect_crypto_rates(timestamp)
            collected_data.extend(crypto_rates)
            
            self.logger.info(f"Successfully collected {len(collected_data)} exchange rates")
            
            return CollectionResult(
                success=True,
                data=collected_data,
                collected_at=timestamp
            )
            
        except Exception as e:
            self.logger.error(f"Error collecting exchange rates: {e}", exc_info=True)
            return CollectionResult(
                success=False,
                error=str(e)
            )
    
    async def _collect_fiat_rates(self, timestamp: datetime) -> List[Dict[str, Any]]:
        """
        采集法币汇率
        
        使用免费的ExchangeRate API
        
        Returns:
            List[Dict]: 法币汇率数据
        """
        rates = []
        
        try:
            async with aiohttp.ClientSession() as session:
                async with session.get(self.fiat_api_url, timeout=10) as response:
                    if response.status != 200:
                        self.logger.warning(f"Fiat API returned status {response.status}")
                        return self._get_fallback_fiat_rates(timestamp)
                    
                    data = await response.json()
                    
                    if 'rates' not in data:
                        self.logger.warning("No rates in API response")
                        return self._get_fallback_fiat_rates(timestamp)
                    
                    api_rates = data['rates']
                    
                    # 提取需要的货币对
                    for base, target in self.CURRENCY_PAIRS:
                        if target in api_rates:
                            rate_value = api_rates[target]
                            
                            rates.append({
                                'base_currency': base,
                                'target_currency': target,
                                'exchange_rate': self._parse_decimal(rate_value),
                                'snapshot_at': timestamp,
                                'source': 'exchangerate-api.com'
                            })
                    
                    self.logger.info(f"Collected {len(rates)} fiat rates")
                    
        except Exception as e:
            self.logger.error(f"Error collecting fiat rates: {e}")
            return self._get_fallback_fiat_rates(timestamp)
        
        return rates
    
    async def _collect_crypto_rates(self, timestamp: datetime) -> List[Dict[str, Any]]:
        """
        采集加密货币价格
        
        使用CoinGecko免费API
        
        Returns:
            List[Dict]: 加密货币价格数据
        """
        rates = []
        
        try:
            # CoinGecko币种ID映射
            coin_ids = {
                'BTC': 'bitcoin',
                'ETH': 'ethereum',
                'BNB': 'binancecoin',
                'USDT': 'tether',
                'USDC': 'usd-coin'
            }
            
            # 构建请求参数
            ids_param = ','.join(coin_ids.values())
            params = {
                'ids': ids_param,
                'vs_currencies': 'usd'
            }
            
            async with aiohttp.ClientSession() as session:
                async with session.get(
                    self.crypto_api_url,
                    params=params,
                    timeout=10
                ) as response:
                    if response.status != 200:
                        self.logger.warning(f"Crypto API returned status {response.status}")
                        return self._get_fallback_crypto_rates(timestamp)
                    
                    data = await response.json()
                    
                    # 提取价格
                    for symbol, coin_id in coin_ids.items():
                        if coin_id in data and 'usd' in data[coin_id]:
                            price = data[coin_id]['usd']
                            
                            rates.append({
                                'base_currency': symbol,
                                'target_currency': 'USD',
                                'exchange_rate': self._parse_decimal(price),
                                'snapshot_at': timestamp,
                                'source': 'coingecko.com'
                            })
                    
                    self.logger.info(f"Collected {len(rates)} crypto rates")
                    
        except Exception as e:
            self.logger.error(f"Error collecting crypto rates: {e}")
            return self._get_fallback_crypto_rates(timestamp)
        
        return rates
    
    def _get_fallback_fiat_rates(self, timestamp: datetime) -> List[Dict[str, Any]]:
        """
        获取备用法币汇率 (API失败时使用)
        
        使用最近的历史平均值
        """
        self.logger.info("Using fallback fiat rates")
        
        fallback_rates = {
            ('USD', 'CNY'): Decimal('7.2'),
            ('USD', 'EUR'): Decimal('0.92'),
            ('USD', 'JPY'): Decimal('149.5'),
        }
        
        return [
            {
                'base_currency': base,
                'target_currency': target,
                'exchange_rate': rate,
                'snapshot_at': timestamp,
                'source': 'fallback'
            }
            for (base, target), rate in fallback_rates.items()
        ]
    
    def _get_fallback_crypto_rates(self, timestamp: datetime) -> List[Dict[str, Any]]:
        """
        获取备用加密货币价格 (API失败时使用)
        
        使用最近的历史平均值
        """
        self.logger.info("Using fallback crypto rates")
        
        fallback_rates = {
            'BTC': Decimal('65000'),
            'ETH': Decimal('3500'),
            'BNB': Decimal('600'),
            'USDT': Decimal('1.0'),
            'USDC': Decimal('1.0'),
        }
        
        return [
            {
                'base_currency': symbol,
                'target_currency': 'USD',
                'exchange_rate': rate,
                'snapshot_at': timestamp,
                'source': 'fallback'
            }
            for symbol, rate in fallback_rates.items()
        ]
    
    async def save_to_db(self, result: CollectionResult) -> bool:
        """
        将汇率数据保存到数据库
        
        保存到的表:
        - rate_snapshots - 汇率快照
        
        Args:
            result: 采集结果
            
        Returns:
            bool: 是否保存成功
        """
        db: Optional[Session] = None
        
        try:
            db = SessionLocal()
            
            saved_count = 0
            
            for item in result.data:
                # 创建汇率快照记录
                rate_snapshot = RateSnapshot(
                    base_currency=item['base_currency'],
                    target_currency=item['target_currency'],
                    exchange_rate=item['exchange_rate'],
                    snapshot_at=item['snapshot_at'],
                    source=item.get('source', 'unknown')
                )
                
                db.add(rate_snapshot)
                saved_count += 1
            
            db.commit()
            self.logger.info(f"Saved {saved_count} exchange rate records to database")
            
            return saved_count > 0
            
        except Exception as e:
            self.logger.error(f"Error saving to database: {e}", exc_info=True)
            if db:
                db.rollback()
            return False
            
        finally:
            if db:
                db.close()
    
    async def validate(self, data: Dict[str, Any]) -> bool:
        """
        验证单条汇率数据的有效性
        
        必须字段:
        - base_currency
        - target_currency
        - exchange_rate
        - snapshot_at
        
        Args:
            data: 单条数据
            
        Returns:
            bool: 数据是否有效
        """
        required_fields = ['base_currency', 'target_currency', 'exchange_rate', 'snapshot_at']
        
        for field in required_fields:
            if field not in data or data[field] is None:
                self.logger.warning(f"Missing required field: {field}")
                return False
        
        # exchange_rate必须是正数
        rate = data['exchange_rate']
        if not isinstance(rate, (int, float, Decimal)) or rate <= 0:
            self.logger.warning(f"Invalid exchange_rate: {rate}")
            return False
        
        return True
    
    async def get_rate(
        self,
        base: str,
        target: str,
        timestamp: Optional[datetime] = None
    ) -> Optional[Decimal]:
        """
        获取特定货币对的汇率
        
        Args:
            base: 基础货币
            target: 目标货币
            timestamp: 时间戳 (None表示最新)
            
        Returns:
            Decimal or None: 汇率值
        """
        try:
            # 如果没有指定时间，采集最新数据
            if timestamp is None:
                timestamp = datetime.utcnow()
            
            result = await self.collect(timestamp)
            
            if not result.success:
                return None
            
            # 查找对应的汇率
            for item in result.data:
                if (item['base_currency'] == base.upper() and 
                    item['target_currency'] == target.upper()):
                    return item['exchange_rate']
            
            return None
            
        except Exception as e:
            self.logger.error(f"Error getting exchange rate {base}/{target}: {e}")
            return None
