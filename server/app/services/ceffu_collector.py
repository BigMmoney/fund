"""
Ceffu数据采集器
Ceffu Data Collector

负责从Ceffu API采集钱包资产数据，包括:
- 钱包列表
- 各钱包的资产余额
- 资产USD价值
"""

from typing import List, Dict, Any, Optional
from datetime import datetime
from decimal import Decimal
from sqlalchemy.orm import Session
import logging

from server.app.services.data_collector import DataCollector, CollectionResult
from server.app.services.ceffu_client import CeffuClient
from server.app.models.snapshots import AssetsSnapshot
from server.app.database import SessionLocal

logger = logging.getLogger(__name__)


class CeffuCollector(DataCollector):
    """
    Ceffu数据采集器
    
    采集钱包资产数据并保存到数据库
    """
    
    def __init__(self, ceffu_client: CeffuClient):
        super().__init__("CeffuCollector")
        self.client = ceffu_client
    
    async def collect(self, timestamp: datetime) -> CollectionResult:
        """
        从Ceffu API采集钱包资产数据
        
        Steps:
        1. 获取所有钱包列表
        2. 对每个钱包获取资产详情
        3. 计算资产USD价值
        
        Returns:
            CollectionResult: 包含所有资产数据
        """
        try:
            self.logger.info(f"Collecting Ceffu data at {timestamp}")
            
            # 获取钱包列表
            wallets_response = self.client.get_wallet_list()
            
            if not wallets_response or 'data' not in wallets_response:
                return CollectionResult(
                    success=False,
                    error="Failed to get wallets list"
                )
            
            wallets = wallets_response.get('data', {}).get('data', [])
            
            if not wallets:
                self.logger.warning("No wallets found")
                return CollectionResult(
                    success=True,
                    data=[],
                    collected_at=timestamp
                )
            
            # 采集每个钱包的资产数据
            collected_data = []
            
            for wallet in wallets:
                wallet_id = wallet.get('walletId')
                wallet_name = wallet.get('walletName')
                
                if not wallet_id:
                    self.logger.warning(f"Invalid wallet data: {wallet}")
                    continue
                
                # 获取钱包资产
                assets_response = self.client.get_wallet_asset_list(str(wallet_id))
                
                if not assets_response or 'data' not in assets_response:
                    self.logger.warning(f"Failed to get assets for wallet {wallet_id}")
                    continue
                
                assets = assets_response.get('data', {}).get('data', [])
                
                # 处理每个资产
                for asset in assets:
                    coin_symbol = asset.get('coinSymbol')
                    network = asset.get('network')
                    
                    # ⭐ 使用 totalAmountWithMirror 获取包含MirrorX的总余额
                    # amount: Ceffu托管中的余额
                    # totalAmountWithMirror: 包含委托到Binance的总余额
                    total_amount_with_mirror = asset.get('totalAmountWithMirror')
                    amount = asset.get('amount')
                    available_amount = asset.get('availableAmount')
                    
                    # 优先使用totalAmountWithMirror，如果没有则使用amount
                    actual_amount = total_amount_with_mirror if total_amount_with_mirror is not None else amount
                    
                    if not coin_symbol or actual_amount is None:
                        continue
                    
                    # 计算USD价值 (使用实际余额)
                    usd_value = await self._calculate_usd_value(
                        coin_symbol,
                        actual_amount,
                        timestamp
                    )
                    
                    item = {
                        'wallet_id': wallet_id,
                        'wallet_name': wallet_name,
                        'coin_symbol': coin_symbol,
                        'network': network,
                        'amount': self._parse_decimal(actual_amount),  # 使用totalAmountWithMirror
                        'available_amount': self._parse_decimal(available_amount),
                        'amount_in_custody': self._parse_decimal(amount),  # Ceffu托管中的余额
                        'usd_value': usd_value,
                        'snapshot_at': timestamp
                    }
                    
                    collected_data.append(item)
                
                self.logger.debug(f"Collected {len(assets)} assets for wallet {wallet_id}")
            
            self.logger.info(f"Successfully collected {len(collected_data)} asset records")
            
            return CollectionResult(
                success=True,
                data=collected_data,
                collected_at=timestamp
            )
            
        except Exception as e:
            self.logger.error(f"Error collecting Ceffu data: {e}", exc_info=True)
            return CollectionResult(
                success=False,
                error=str(e)
            )
    
    async def save_to_db(self, result: CollectionResult) -> bool:
        """
        将采集的资产数据保存到数据库
        
        保存到的表:
        - assets_snapshots - 资产快照
        
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
                # 创建资产快照记录
                asset_snapshot = AssetsSnapshot(
                    snapshot_at=int(item['snapshot_at'].timestamp()),  # 转换为秒级时间戳
                    wallet_id=item['wallet_id'],
                    asset_symbol=item['coin_symbol'],
                    balance=item['amount'],
                    assets_value=str(item.get('usd_value') or Decimal('0'))
                )
                
                db.add(asset_snapshot)
                saved_count += 1
            
            db.commit()
            self.logger.info(f"Saved {saved_count} asset records to database")
            
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
        验证单条资产数据的有效性
        
        必须字段:
        - wallet_id
        - coin_symbol
        - amount
        - snapshot_at
        
        Args:
            data: 单条数据
            
        Returns:
            bool: 数据是否有效
        """
        required_fields = ['wallet_id', 'coin_symbol', 'amount', 'snapshot_at']
        
        for field in required_fields:
            if field not in data or data[field] is None:
                self.logger.warning(f"Missing required field: {field}")
                return False
        
        # amount必须是数字类型
        if not isinstance(data['amount'], (int, float, Decimal)):
            self.logger.warning(f"Invalid amount type: {type(data['amount'])}")
            return False
        
        # amount不能是负数
        if data['amount'] < 0:
            self.logger.warning(f"Negative amount: {data['amount']}")
            return False
        
        return True
    
    async def _calculate_usd_value(
        self,
        coin_symbol: str,
        amount: Any,
        timestamp: datetime
    ) -> Optional[Decimal]:
        """
        计算资产的USD价值
        
        Steps:
        1. 如果是稳定币 (USDT, USDC, USD1)，直接返回amount
        2. 否则，查询价格API获取USD价格
        3. 计算: usd_value = amount × price
        
        Args:
            coin_symbol: 币种符号
            amount: 数量
            timestamp: 时间戳
            
        Returns:
            Decimal or None
        """
        try:
            amount_decimal = self._parse_decimal(amount)
            
            if amount_decimal is None or amount_decimal == 0:
                return Decimal('0')
            
            # 稳定币直接返回
            stable_coins = ['USDT', 'USDC', 'USD1', 'BUSD', 'DAI']
            if coin_symbol.upper() in stable_coins:
                return amount_decimal
            
            # 获取价格 (这里需要实现价格查询逻辑)
            # 暂时返回None，后续需要集成价格API
            price = await self._get_price(coin_symbol, timestamp)
            
            if price is None:
                self.logger.warning(f"No price available for {coin_symbol}")
                return None
            
            usd_value = amount_decimal * price
            
            return usd_value
            
        except Exception as e:
            self.logger.error(f"Error calculating USD value for {coin_symbol}: {e}")
            return None
    
    async def _get_price(self, coin_symbol: str, timestamp: datetime) -> Optional[Decimal]:
        """
        获取币种的USD价格
        
        TODO: 实现价格查询逻辑
        - 可以集成CoinGecko API
        - 可以集成Binance价格API
        - 可以查询内部价格表
        
        Args:
            coin_symbol: 币种符号
            timestamp: 时间戳
            
        Returns:
            Decimal or None
        """
        # 临时实现：返回预设价格
        price_map = {
            'BTC': Decimal('65000'),
            'ETH': Decimal('3500'),
            'BNB': Decimal('600'),
            'SOL': Decimal('150'),
        }
        
        return price_map.get(coin_symbol.upper())
    
    async def get_total_assets_usd(self, timestamp: datetime) -> Optional[Decimal]:
        """
        获取所有钱包的资产总值 (USD)
        
        用于生成整体净资产快照
        
        Args:
            timestamp: 时间戳
            
        Returns:
            Decimal or None: 总资产USD价值
        """
        try:
            result = await self.collect(timestamp)
            
            if not result.success:
                return None
            
            total_usd = Decimal('0')
            
            for item in result.data:
                if item.get('usd_value'):
                    total_usd += item['usd_value']
            
            return total_usd
            
        except Exception as e:
            self.logger.error(f"Error calculating total assets: {e}")
            return None
