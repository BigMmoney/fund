"""
P1+ Enhanced Scheduler
增强的数据采集调度器

集成了新的数据采集器:
- OneToken投资组合采集器
- Ceffu钱包资产采集器
- 汇率数据采集器
"""

import asyncio
import logging
from datetime import datetime, timedelta
from typing import Optional, Dict, List
from sqlalchemy.orm import Session

from server.app.database import SessionLocal
from server.app.services.data_collector import collector_registry
from server.app.services.onetoken_collector import OneTokenCollector
from server.app.services.ceffu_collector import CeffuCollector
from server.app.services.exchange_rate_collector import ExchangeRateCollector
from server.app.services.onetoken_client import OneTokenClient
from server.app.services.ceffu_client import CeffuClient
from server.app.config import settings

logger = logging.getLogger(__name__)


class DataCollectionScheduler:
    """
    P1+ 数据采集调度器
    
    功能:
    1. 每小时定时采集数据
    2. 管理多个采集器
    3. 错误处理与重试
    4. 采集结果统计
    """
    
    def __init__(self):
        self.running = False
        self.tasks: List[asyncio.Task] = []
        self.collectors_initialized = False
        
        # 统计信息
        self.stats = {
            'total_runs': 0,
            'successful_runs': 0,
            'failed_runs': 0,
            'last_run_time': None,
            'last_run_success': None
        }
    
    async def initialize_collectors(self):
        """
        初始化所有数据采集器
        
        创建并注册:
        - OneToken采集器
        - Ceffu采集器
        - 汇率采集器
        """
        if self.collectors_initialized:
            logger.warning("Collectors already initialized")
            return
        
        try:
            logger.info("Initializing data collectors...")
            
            # 初始化OneToken客户端和采集器
            onetoken_client = OneTokenClient(
                api_key=settings.onetoken_api_key,
                secret=settings.onetoken_secret
            )
            onetoken_collector = OneTokenCollector(onetoken_client)
            collector_registry.register(onetoken_collector)
            logger.info("✅ OneToken collector registered")
            
            # 初始化Ceffu客户端和采集器
            ceffu_client = CeffuClient(
                public_key=settings.ceffu_public_key,
                private_key=settings.ceffu_private_key
            )
            ceffu_collector = CeffuCollector(ceffu_client)
            collector_registry.register(ceffu_collector)
            logger.info("✅ Ceffu collector registered")
            
            # 初始化汇率采集器
            exchange_rate_collector = ExchangeRateCollector()
            collector_registry.register(exchange_rate_collector)
            logger.info("✅ Exchange rate collector registered")
            
            self.collectors_initialized = True
            logger.info(f"All collectors initialized ({len(collector_registry.get_all())} total)")
            
        except Exception as e:
            logger.error(f"Failed to initialize collectors: {e}", exc_info=True)
            raise
    
    async def start(self):
        """启动调度器"""
        if self.running:
            logger.warning("Scheduler is already running")
            return
        
        # 初始化采集器
        if not self.collectors_initialized:
            await self.initialize_collectors()
        
        self.running = True
        logger.info("🚀 Starting P1+ data collection scheduler")
        
        # 启动每小时采集任务
        hourly_task = asyncio.create_task(self._schedule_hourly_collection())
        self.tasks.append(hourly_task)
        
        logger.info("✅ P1+ scheduler started successfully")
    
    async def stop(self):
        """停止调度器"""
        if not self.running:
            return
        
        self.running = False
        logger.info("Stopping P1+ scheduler...")
        
        # 取消所有任务
        for task in self.tasks:
            task.cancel()
        
        # 等待任务完成
        await asyncio.gather(*self.tasks, return_exceptions=True)
        self.tasks.clear()
        
        logger.info("✅ P1+ scheduler stopped")
    
    async def _schedule_hourly_collection(self):
        """
        每小时数据采集调度
        
        在每小时的第5分钟开始采集 (给API一些时间准备数据)
        """
        while self.running:
            try:
                # 计算下次采集时间 (下一个小时的第5分钟)
                now = datetime.now()
                next_hour = (now + timedelta(hours=1)).replace(minute=5, second=0, microsecond=0)
                
                # 如果当前已经过了第5分钟，则等到下个小时
                if now.minute >= 5:
                    next_hour = (now + timedelta(hours=1)).replace(minute=5, second=0, microsecond=0)
                else:
                    next_hour = now.replace(minute=5, second=0, microsecond=0)
                
                wait_seconds = (next_hour - now).total_seconds()
                
                logger.info(f"⏰ Next data collection in {wait_seconds:.0f} seconds ({next_hour.strftime('%Y-%m-%d %H:%M')})")
                
                await asyncio.sleep(wait_seconds)
                
                if self.running:
                    # 采集时间戳为整点时间
                    collection_timestamp = next_hour.replace(minute=0)
                    await self._run_collection(collection_timestamp)
                    
            except asyncio.CancelledError:
                break
            except Exception as e:
                logger.error(f"Error in hourly collection schedule: {e}", exc_info=True)
                await asyncio.sleep(300)  # 等待5分钟后重试
    
    async def _run_collection(self, timestamp: datetime):
        """
        运行一次完整的数据采集
        
        Steps:
        1. 运行所有注册的采集器
        2. 记录结果
        3. 更新统计信息
        
        Args:
            timestamp: 采集的时间戳 (整点时间)
        """
        try:
            logger.info(f"=" * 80)
            logger.info(f"📊 Starting data collection for {timestamp.strftime('%Y-%m-%d %H:%M')}")
            logger.info(f"=" * 80)
            
            self.stats['total_runs'] += 1
            self.stats['last_run_time'] = datetime.now()
            
            # 运行所有采集器
            results = await collector_registry.run_all(timestamp)
            
            # 统计结果
            total_collectors = len(results)
            successful = sum(1 for success in results.values() if success)
            failed = total_collectors - successful
            
            # 记录详细结果
            logger.info(f"\n📈 Collection Results:")
            logger.info(f"   Total collectors: {total_collectors}")
            logger.info(f"   ✅ Successful: {successful}")
            logger.info(f"   ❌ Failed: {failed}")
            
            for name, success in results.items():
                status = "✅ SUCCESS" if success else "❌ FAILED"
                logger.info(f"   - {name}: {status}")
            
            # 更新统计
            if failed == 0:
                self.stats['successful_runs'] += 1
                self.stats['last_run_success'] = True
                logger.info(f"\n🎉 Data collection completed successfully!")
            else:
                self.stats['failed_runs'] += 1
                self.stats['last_run_success'] = False
                logger.warning(f"\n⚠️  Data collection completed with {failed} failures")
            
            logger.info(f"=" * 80)
            
        except Exception as e:
            logger.error(f"❌ Error in data collection: {e}", exc_info=True)
            self.stats['failed_runs'] += 1
            self.stats['last_run_success'] = False
    
    async def run_manual_collection(self, timestamp: Optional[datetime] = None) -> Dict[str, bool]:
        """
        手动触发一次数据采集
        
        Args:
            timestamp: 采集时间戳 (None表示使用当前时间)
            
        Returns:
            Dict[str, bool]: 每个采集器的结果
        """
        if not self.collectors_initialized:
            await self.initialize_collectors()
        
        if timestamp is None:
            timestamp = datetime.now().replace(minute=0, second=0, microsecond=0)
        
        logger.info(f"🔧 Manual collection triggered for {timestamp.strftime('%Y-%m-%d %H:%M')}")
        
        await self._run_collection(timestamp)
        
        # 返回最近一次运行的结果
        return self.get_stats()
    
    def get_stats(self) -> Dict:
        """
        获取调度器统计信息
        
        Returns:
            Dict: 统计数据
        """
        return {
            'total_runs': self.stats['total_runs'],
            'successful_runs': self.stats['successful_runs'],
            'failed_runs': self.stats['failed_runs'],
            'success_rate': (
                f"{(self.stats['successful_runs'] / self.stats['total_runs'] * 100):.1f}%"
                if self.stats['total_runs'] > 0 else "N/A"
            ),
            'last_run_time': (
                self.stats['last_run_time'].strftime('%Y-%m-%d %H:%M:%S')
                if self.stats['last_run_time'] else None
            ),
            'last_run_success': self.stats['last_run_success'],
            'collectors_count': len(collector_registry.get_all()),
            'running': self.running
        }


# 全局调度器实例
p1_scheduler = DataCollectionScheduler()


async def start_p1_scheduler():
    """启动P1+调度器"""
    await p1_scheduler.start()


async def stop_p1_scheduler():
    """停止P1+调度器"""
    await p1_scheduler.stop()


async def trigger_manual_collection(timestamp: Optional[datetime] = None) -> Dict[str, bool]:
    """手动触发数据采集"""
    return await p1_scheduler.run_manual_collection(timestamp)


def get_scheduler_stats() -> Dict:
    """获取调度器统计信息"""
    return p1_scheduler.get_stats()
