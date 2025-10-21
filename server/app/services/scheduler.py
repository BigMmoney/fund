"""
Scheduler service for automatic data collection
"""
import asyncio
import logging
from datetime import datetime, timedelta
from typing import Optional
from sqlalchemy.orm import Session

from app.database import SessionLocal
from app.api.routers.snapshots import collect_all_snapshots
from app.config import settings

logger = logging.getLogger(__name__)


class SnapshotScheduler:
    """Scheduler for automatic snapshot collection"""
    
    def __init__(self):
        self.running = False
        self.tasks = []
    
    async def start(self):
        """Start the scheduler"""
        if self.running:
            logger.warning("Scheduler is already running")
            return
        
        self.running = True
        logger.info("Starting snapshot scheduler")
        
        # Schedule hourly snapshots
        hourly_task = asyncio.create_task(self._schedule_hourly_snapshots())
        self.tasks.append(hourly_task)
        
        # Schedule daily snapshots
        daily_task = asyncio.create_task(self._schedule_daily_snapshots())
        self.tasks.append(daily_task)
        
        # Schedule weekly snapshots
        weekly_task = asyncio.create_task(self._schedule_weekly_snapshots())
        self.tasks.append(weekly_task)
        
        # Schedule monthly snapshots
        monthly_task = asyncio.create_task(self._schedule_monthly_snapshots())
        self.tasks.append(monthly_task)
        
        logger.info("All snapshot schedules started")
    
    async def stop(self):
        """Stop the scheduler"""
        if not self.running:
            return
        
        self.running = False
        logger.info("Stopping snapshot scheduler")
        
        # Cancel all tasks
        for task in self.tasks:
            task.cancel()
        
        # Wait for tasks to complete
        await asyncio.gather(*self.tasks, return_exceptions=True)
        self.tasks.clear()
        
        logger.info("Snapshot scheduler stopped")
    
    async def _schedule_hourly_snapshots(self):
        """Schedule hourly snapshot collection"""
        while self.running:
            try:
                # Wait until the next hour
                now = datetime.now()
                next_hour = (now + timedelta(hours=1)).replace(minute=0, second=0, microsecond=0)
                wait_seconds = (next_hour - now).total_seconds()
                
                logger.info(f"Next hourly snapshot in {wait_seconds:.0f} seconds")
                await asyncio.sleep(wait_seconds)
                
                if self.running:
                    await self._collect_snapshots("hourly")
                    
            except asyncio.CancelledError:
                break
            except Exception as e:
                logger.error(f"Error in hourly snapshot schedule: {e}")
                await asyncio.sleep(300)  # Wait 5 minutes before retry
    
    async def _schedule_daily_snapshots(self):
        """Schedule daily snapshot collection at midnight"""
        while self.running:
            try:
                # Wait until midnight
                now = datetime.now()
                next_midnight = (now + timedelta(days=1)).replace(hour=0, minute=0, second=0, microsecond=0)
                wait_seconds = (next_midnight - now).total_seconds()
                
                logger.info(f"Next daily snapshot in {wait_seconds:.0f} seconds")
                await asyncio.sleep(wait_seconds)
                
                if self.running:
                    await self._collect_snapshots("daily")
                    
            except asyncio.CancelledError:
                break
            except Exception as e:
                logger.error(f"Error in daily snapshot schedule: {e}")
                await asyncio.sleep(3600)  # Wait 1 hour before retry
    
    async def _schedule_weekly_snapshots(self):
        """Schedule weekly snapshot collection on Sundays at midnight"""
        while self.running:
            try:
                # Wait until next Sunday midnight
                now = datetime.now()
                days_until_sunday = (6 - now.weekday()) % 7
                if days_until_sunday == 0 and now.hour == 0 and now.minute < 5:
                    days_until_sunday = 7  # If it's Sunday morning, wait for next Sunday
                
                next_sunday = (now + timedelta(days=days_until_sunday)).replace(hour=0, minute=0, second=0, microsecond=0)
                wait_seconds = (next_sunday - now).total_seconds()
                
                logger.info(f"Next weekly snapshot in {wait_seconds:.0f} seconds")
                await asyncio.sleep(wait_seconds)
                
                if self.running:
                    await self._collect_snapshots("weekly")
                    
            except asyncio.CancelledError:
                break
            except Exception as e:
                logger.error(f"Error in weekly snapshot schedule: {e}")
                await asyncio.sleep(3600)  # Wait 1 hour before retry
    
    async def _schedule_monthly_snapshots(self):
        """Schedule monthly snapshot collection on the 1st at midnight"""
        while self.running:
            try:
                # Wait until first day of next month
                now = datetime.now()
                if now.day == 1 and now.hour == 0 and now.minute < 5:
                    # If it's already the 1st, wait for next month
                    if now.month == 12:
                        next_first = datetime(now.year + 1, 1, 1)
                    else:
                        next_first = datetime(now.year, now.month + 1, 1)
                else:
                    # Calculate next first day of month
                    if now.month == 12:
                        next_first = datetime(now.year + 1, 1, 1)
                    else:
                        next_first = datetime(now.year, now.month + 1, 1)
                
                wait_seconds = (next_first - now).total_seconds()
                
                logger.info(f"Next monthly snapshot in {wait_seconds:.0f} seconds")
                await asyncio.sleep(wait_seconds)
                
                if self.running:
                    await self._collect_snapshots("monthly")
                    
            except asyncio.CancelledError:
                break
            except Exception as e:
                logger.error(f"Error in monthly snapshot schedule: {e}")
                await asyncio.sleep(3600)  # Wait 1 hour before retry
    
    async def _collect_snapshots(self, snapshot_type: str):
        """Collect snapshots of specified type - 优化内存管理"""
        db = None
        try:
            db = SessionLocal()
            logger.info(f"Starting {snapshot_type} snapshot collection")
            await collect_all_snapshots(db, snapshot_type)
            logger.info(f"Completed {snapshot_type} snapshot collection")
            
        except Exception as e:
            logger.error(f"Error collecting {snapshot_type} snapshots: {e}")
            if db:
                db.rollback()
        finally:
            if db:
                db.close()
                db = None  # 显式释放引用
            
            # 强制垃圾回收，释放内存
            import gc
            gc.collect()
            logger.debug(f"Memory cleanup after {snapshot_type} snapshot")


# Global scheduler instance
scheduler = SnapshotScheduler()


async def start_scheduler():
    """Start the global scheduler"""
    await scheduler.start()


async def stop_scheduler():
    """Stop the global scheduler"""
    await scheduler.stop()


async def collect_snapshot_manually(snapshot_type: str, portfolio_id: Optional[int] = None):
    """Manually trigger snapshot collection"""
    db = SessionLocal()
    try:
        logger.info(f"Manual {snapshot_type} snapshot collection triggered")
        await collect_all_snapshots(db, snapshot_type, portfolio_id)
        logger.info(f"Manual {snapshot_type} snapshot collection completed")
        return True
        
    except Exception as e:
        logger.error(f"Error in manual snapshot collection: {e}")
        return False
    finally:
        db.close()