"""
OneToken数据采集器
OneToken Data Collector

负责从OneToken API采集投资组合数据，包括:
- 投资组合列表
- 累计收益 (acc_profit)
- NAV数据
"""

from typing import List, Dict, Any, Optional
from datetime import datetime
from decimal import Decimal
from sqlalchemy.orm import Session
import logging

from server.app.services.data_collector import DataCollector, CollectionResult
from server.app.services.onetoken_client import OneTokenClient
from server.app.models.profit import AccProfitFromPortfolio
from server.app.models.snapshots import NavSnapshot
from server.app.models.portfolio import Portfolio
from server.app.database import SessionLocal

logger = logging.getLogger(__name__)


class OneTokenCollector(DataCollector):
    """
    OneToken数据采集器
    
    采集投资组合的累计收益数据并保存到数据库
    """
    
    def __init__(self, onetoken_client: OneTokenClient):
        super().__init__("OneTokenCollector")
        self.client = onetoken_client
    
    async def collect(self, timestamp: datetime) -> CollectionResult:
        """
        从OneToken API采集投资组合数据
        
        Steps:
        1. 获取所有投资组合列表
        2. 对每个组合获取详细信息 (包括累计收益)
        3. 提取关键数据
        
        Returns:
            CollectionResult: 包含所有组合数据
        """
        try:
            self.logger.info(f"Collecting OneToken data at {timestamp}")
            
            # 获取投资组合列表
            portfolios_response = await self.client.get_portfolios()
            
            if not portfolios_response or 'data' not in portfolios_response:
                return CollectionResult(
                    success=False,
                    error="Failed to get portfolios list"
                )
            
            portfolios = portfolios_response['data'].get('result', [])
            
            if not portfolios:
                self.logger.warning("No portfolios found")
                return CollectionResult(
                    success=True,
                    data=[],
                    collected_at=timestamp
                )
            
            # 采集每个组合的详细数据
            collected_data = []
            
            for portfolio in portfolios:
                portfolio_name = portfolio.get('name')
                ot_symbol = portfolio.get('ot_symbol')
                
                if not portfolio_name or not ot_symbol:
                    self.logger.warning(f"Invalid portfolio data: {portfolio}")
                    continue
                
                # 获取组合详情
                detail = await self.client.get_portfolio_detail(ot_symbol)
                
                if not detail or 'data' not in detail:
                    self.logger.warning(f"Failed to get detail for {portfolio_name}")
                    continue
                
                portfolio_detail = detail['data']
                
                # 提取关键数据
                item = {
                    'fund_name': portfolio_name,
                    'ot_symbol': ot_symbol,
                    'snapshot_at': timestamp,
                    'acc_profit': self._extract_acc_profit(portfolio_detail),
                    'nav': self._extract_nav(portfolio_detail),
                    'status': portfolio_detail.get('status', 'unknown'),
                    'quote': portfolio_detail.get('quote', 'unknown'),
                    'inception_time': self._parse_timestamp(portfolio_detail.get('inception_time'))
                }
                
                collected_data.append(item)
                self.logger.debug(f"Collected data for {portfolio_name}")
            
            self.logger.info(f"Successfully collected {len(collected_data)} portfolios")
            
            return CollectionResult(
                success=True,
                data=collected_data,
                collected_at=timestamp
            )
            
        except Exception as e:
            self.logger.error(f"Error collecting OneToken data: {e}", exc_info=True)
            return CollectionResult(
                success=False,
                error=str(e)
            )
    
    async def save_to_db(self, result: CollectionResult) -> bool:
        """
        将采集的数据保存到数据库
        
        保存到的表:
        1. acc_profit_from_portfolio - 投资组合累计收益
        2. nav_snapshots - NAV快照
        3. portfolios - 更新投资组合基础信息 (如果需要)
        
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
                # 查找或创建Portfolio记录
                portfolio = self._get_or_create_portfolio(db, item)
                
                if not portfolio:
                    self.logger.warning(f"Failed to get/create portfolio for {item['fund_name']}")
                    continue
                
                # 保存累计收益快照
                if item.get('acc_profit') is not None:
                    acc_profit_snapshot = AccProfitFromPortfolio(
                        portfolio_id=portfolio.id,
                        snapshot_at=item['snapshot_at'],
                        acc_profit=item['acc_profit'],
                        created_at=datetime.utcnow()
                    )
                    
                    db.add(acc_profit_snapshot)
                    saved_count += 1
                
                # 保存NAV快照
                if item.get('nav') is not None:
                    nav_snapshot = NavSnapshot(
                        portfolio_id=portfolio.id,
                        snapshot_at=item['snapshot_at'],
                        nav=item['nav'],
                        created_at=datetime.utcnow()
                    )
                    
                    db.add(nav_snapshot)
            
            db.commit()
            self.logger.info(f"Saved {saved_count} records to database")
            
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
        验证单条数据的有效性
        
        必须字段:
        - fund_name
        - ot_symbol
        - snapshot_at
        - acc_profit (可以是0但不能None)
        
        Args:
            data: 单条数据
            
        Returns:
            bool: 数据是否有效
        """
        required_fields = ['fund_name', 'ot_symbol', 'snapshot_at']
        
        for field in required_fields:
            if field not in data or data[field] is None:
                self.logger.warning(f"Missing required field: {field}")
                return False
        
        # acc_profit可以是0，但必须存在且是数字类型
        if 'acc_profit' not in data:
            self.logger.warning("Missing acc_profit field")
            return False
        
        if not isinstance(data['acc_profit'], (int, float, Decimal)) and data['acc_profit'] is not None:
            self.logger.warning(f"Invalid acc_profit type: {type(data['acc_profit'])}")
            return False
        
        return True
    
    def _extract_acc_profit(self, portfolio_detail: Dict[str, Any]) -> Optional[Decimal]:
        """
        从投资组合详情中提取累计收益
        
        OneToken可能返回的字段:
        - acc_profit
        - accumulated_profit
        - total_profit
        
        Args:
            portfolio_detail: 组合详情
            
        Returns:
            Decimal or None
        """
        for field in ['acc_profit', 'accumulated_profit', 'total_profit', 'pnl']:
            if field in portfolio_detail:
                return self._parse_decimal(portfolio_detail[field])
        
        return None
    
    def _extract_nav(self, portfolio_detail: Dict[str, Any]) -> Optional[Decimal]:
        """
        从投资组合详情中提取NAV值
        
        Args:
            portfolio_detail: 组合详情
            
        Returns:
            Decimal or None
        """
        for field in ['nav', 'net_asset_value', 'unit_nav']:
            if field in portfolio_detail:
                return self._parse_decimal(portfolio_detail[field])
        
        return None
    
    def _get_or_create_portfolio(self, db: Session, item: Dict[str, Any]) -> Optional[Portfolio]:
        """
        根据fund_name查找或创建Portfolio记录
        
        Args:
            db: 数据库会话
            item: 数据项
            
        Returns:
            Portfolio or None
        """
        try:
            # 先尝试查找
            portfolio = db.query(Portfolio).filter(
                Portfolio.fund_name == item['fund_name']
            ).first()
            
            if portfolio:
                # 更新基础信息
                if item.get('status'):
                    portfolio.status = item['status']
                if item.get('inception_time'):
                    portfolio.inception_time = item['inception_time']
                
                return portfolio
            
            # 创建新记录
            portfolio = Portfolio(
                fund_name=item['fund_name'],
                fund_alias=item.get('fund_name', ''),  # 可以后续更新
                ot_symbol=item.get('ot_symbol', ''),
                status=item.get('status', 'unknown'),
                inception_time=item.get('inception_time'),
                created_at=datetime.utcnow()
            )
            
            db.add(portfolio)
            db.flush()  # 获取ID但不提交
            
            self.logger.info(f"Created new portfolio: {item['fund_name']}")
            
            return portfolio
            
        except Exception as e:
            self.logger.error(f"Error getting/creating portfolio: {e}")
            return None
