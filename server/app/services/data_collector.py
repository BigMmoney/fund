"""
数据采集器基类
Data Collector Base Class

提供统一的数据采集接口，所有具体采集器都继承此基类
"""

from abc import ABC, abstractmethod
from typing import List, Dict, Any, Optional
from datetime import datetime
from decimal import Decimal
import logging

logger = logging.getLogger(__name__)


class CollectionResult:
    """采集结果封装"""
    
    def __init__(
        self,
        success: bool,
        data: Optional[List[Dict[str, Any]]] = None,
        error: Optional[str] = None,
        collected_at: Optional[datetime] = None
    ):
        self.success = success
        self.data = data or []
        self.error = error
        self.collected_at = collected_at or datetime.utcnow()
        self.count = len(self.data)
    
    def __repr__(self):
        if self.success:
            return f"<CollectionResult success={self.success} count={self.count}>"
        return f"<CollectionResult success={self.success} error={self.error}>"


class DataCollector(ABC):
    """
    数据采集器抽象基类
    
    所有数据采集器必须实现:
    1. collect() - 从外部API采集数据
    2. save_to_db() - 将数据保存到数据库
    3. validate() - 验证采集的数据
    """
    
    def __init__(self, name: str):
        self.name = name
        self.logger = logging.getLogger(f"{__name__}.{name}")
    
    @abstractmethod
    async def collect(self, timestamp: datetime) -> CollectionResult:
        """
        从外部API采集数据
        
        Args:
            timestamp: 采集的时间戳 (通常是整点时间)
            
        Returns:
            CollectionResult: 采集结果
        """
        pass
    
    @abstractmethod
    async def save_to_db(self, result: CollectionResult) -> bool:
        """
        将采集的数据保存到数据库
        
        Args:
            result: 采集结果
            
        Returns:
            bool: 是否保存成功
        """
        pass
    
    @abstractmethod
    async def validate(self, data: Dict[str, Any]) -> bool:
        """
        验证单条数据的有效性
        
        Args:
            data: 单条数据
            
        Returns:
            bool: 数据是否有效
        """
        pass
    
    async def run(self, timestamp: datetime) -> bool:
        """
        执行完整的采集流程
        
        1. 采集数据
        2. 验证数据
        3. 保存到数据库
        
        Args:
            timestamp: 采集时间戳
            
        Returns:
            bool: 整个流程是否成功
        """
        try:
            self.logger.info(f"Starting data collection for {self.name} at {timestamp}")
            
            # 步骤1: 采集数据
            result = await self.collect(timestamp)
            
            if not result.success:
                self.logger.error(f"Data collection failed: {result.error}")
                return False
            
            self.logger.info(f"Collected {result.count} items")
            
            # 步骤2: 验证数据
            valid_data = []
            for item in result.data:
                if await self.validate(item):
                    valid_data.append(item)
                else:
                    self.logger.warning(f"Invalid data item: {item}")
            
            if not valid_data:
                self.logger.error("No valid data after validation")
                return False
            
            result.data = valid_data
            self.logger.info(f"{len(valid_data)} items passed validation")
            
            # 步骤3: 保存到数据库
            save_success = await self.save_to_db(result)
            
            if save_success:
                self.logger.info(f"Successfully saved {len(valid_data)} items to database")
                return True
            else:
                self.logger.error("Failed to save data to database")
                return False
                
        except Exception as e:
            self.logger.error(f"Error in collection workflow: {e}", exc_info=True)
            return False
    
    def _parse_decimal(self, value: Any) -> Optional[Decimal]:
        """
        安全地将值转换为Decimal
        
        Args:
            value: 输入值
            
        Returns:
            Decimal or None
        """
        if value is None:
            return None
        
        try:
            if isinstance(value, Decimal):
                return value
            if isinstance(value, (int, float)):
                return Decimal(str(value))
            if isinstance(value, str):
                # 移除逗号等格式字符
                value = value.replace(',', '').strip()
                return Decimal(value)
            return None
        except Exception as e:
            self.logger.warning(f"Failed to parse decimal from {value}: {e}")
            return None
    
    def _parse_timestamp(self, value: Any) -> Optional[datetime]:
        """
        安全地将值转换为datetime
        
        Args:
            value: 输入值 (可能是时间戳、字符串或datetime对象)
            
        Returns:
            datetime or None
        """
        if value is None:
            return None
        
        try:
            if isinstance(value, datetime):
                return value
            if isinstance(value, (int, float)):
                # Unix时间戳
                return datetime.fromtimestamp(value)
            if isinstance(value, str):
                # ISO格式字符串
                return datetime.fromisoformat(value)
            return None
        except Exception as e:
            self.logger.warning(f"Failed to parse timestamp from {value}: {e}")
            return None


class CollectorRegistry:
    """
    采集器注册表
    
    管理所有注册的数据采集器
    """
    
    def __init__(self):
        self._collectors: Dict[str, DataCollector] = {}
        self.logger = logging.getLogger(__name__)
    
    def register(self, collector: DataCollector):
        """注册一个采集器"""
        self._collectors[collector.name] = collector
        self.logger.info(f"Registered collector: {collector.name}")
    
    def get(self, name: str) -> Optional[DataCollector]:
        """获取指定名称的采集器"""
        return self._collectors.get(name)
    
    def get_all(self) -> List[DataCollector]:
        """获取所有采集器"""
        return list(self._collectors.values())
    
    async def run_all(self, timestamp: datetime) -> Dict[str, bool]:
        """
        运行所有采集器
        
        Args:
            timestamp: 采集时间戳
            
        Returns:
            Dict[str, bool]: 每个采集器的运行结果
        """
        results = {}
        
        for name, collector in self._collectors.items():
            try:
                success = await collector.run(timestamp)
                results[name] = success
            except Exception as e:
                self.logger.error(f"Error running collector {name}: {e}", exc_info=True)
                results[name] = False
        
        return results


# 全局采集器注册表
collector_registry = CollectorRegistry()
