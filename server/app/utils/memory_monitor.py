"""
内存监控和OOM防护工具
"""
import functools
import tracemalloc
import psutil
import os
import logging
from datetime import datetime
from typing import Callable

logger = logging.getLogger(__name__)

class MemoryMonitor:
    """内存监控装饰器"""
    
    @staticmethod
    def monitor(threshold_mb=100):
        """
        监控函数内存使用
        
        Args:
            threshold_mb: 内存使用超过此阈值时发出警告 (MB)
        """
        def decorator(func: Callable):
            @functools.wraps(func)
            async def async_wrapper(*args, **kwargs):
                tracemalloc.start()
                process = psutil.Process(os.getpid())
                mem_before = process.memory_info().rss / 1024 / 1024  # MB
                
                try:
                    result = await func(*args, **kwargs)
                    return result
                finally:
                    current, peak = tracemalloc.get_traced_memory()
                    tracemalloc.stop()
                    
                    mem_after = process.memory_info().rss / 1024 / 1024  # MB
                    mem_used = mem_after - mem_before
                    peak_mb = peak / 1024 / 1024
                    
                    if mem_used > threshold_mb or peak_mb > threshold_mb:
                        logger.warning(
                            f"⚠️  {func.__name__} 内存使用过高: "
                            f"增长={mem_used:.2f}MB, 峰值={peak_mb:.2f}MB"
                        )
                    
                    logger.info(
                        f"📊 {func.__name__}: "
                        f"内存增长={mem_used:.2f}MB, "
                        f"当前={mem_after:.2f}MB, "
                        f"峰值={peak_mb:.2f}MB"
                    )
            
            @functools.wraps(func)
            def sync_wrapper(*args, **kwargs):
                tracemalloc.start()
                process = psutil.Process(os.getpid())
                mem_before = process.memory_info().rss / 1024 / 1024
                
                try:
                    result = func(*args, **kwargs)
                    return result
                finally:
                    current, peak = tracemalloc.get_traced_memory()
                    tracemalloc.stop()
                    
                    mem_after = process.memory_info().rss / 1024 / 1024
                    mem_used = mem_after - mem_before
                    peak_mb = peak / 1024 / 1024
                    
                    if mem_used > threshold_mb or peak_mb > threshold_mb:
                        logger.warning(
                            f"⚠️  {func.__name__} 内存使用过高: "
                            f"增长={mem_used:.2f}MB, 峰值={peak_mb:.2f}MB"
                        )
            
            import asyncio
            if asyncio.iscoroutinefunction(func):
                return async_wrapper
            return sync_wrapper
        
        return decorator
    
    @staticmethod
    def check_system_memory():
        """检查系统内存状态"""
        memory = psutil.virtual_memory()
        swap = psutil.swap_memory()
        
        return {
            "total_mb": memory.total / 1024 / 1024,
            "available_mb": memory.available / 1024 / 1024,
            "percent": memory.percent,
            "swap_percent": swap.percent,
            "process_mb": psutil.Process().memory_info().rss / 1024 / 1024
        }
    
    @staticmethod
    def oom_guard(max_memory_mb=1024):
        """
        OOM保护装饰器
        如果内存使用超过阈值，拒绝执行
        
        Args:
            max_memory_mb: 最大允许内存使用 (MB)
        """
        def decorator(func: Callable):
            @functools.wraps(func)
            async def async_wrapper(*args, **kwargs):
                process = psutil.Process(os.getpid())
                current_memory = process.memory_info().rss / 1024 / 1024
                
                if current_memory > max_memory_mb:
                    from fastapi import HTTPException
                    raise HTTPException(
                        status_code=503,
                        detail=f"服务内存不足: {current_memory:.2f}MB / {max_memory_mb}MB"
                    )
                
                return await func(*args, **kwargs)
            
            @functools.wraps(func)
            def sync_wrapper(*args, **kwargs):
                process = psutil.Process(os.getpid())
                current_memory = process.memory_info().rss / 1024 / 1024
                
                if current_memory > max_memory_mb:
                    raise MemoryError(
                        f"内存使用超限: {current_memory:.2f}MB / {max_memory_mb}MB"
                    )
                
                return func(*args, **kwargs)
            
            import asyncio
            if asyncio.iscoroutinefunction(func):
                return async_wrapper
            return sync_wrapper
        
        return decorator


# 使用示例
if __name__ == "__main__":
    # 示例1: 监控内存使用
    @MemoryMonitor.monitor(threshold_mb=50)
    async def heavy_task():
        data = [i for i in range(1000000)]
        return len(data)
    
    # 示例2: OOM保护
    @MemoryMonitor.oom_guard(max_memory_mb=512)
    async def api_endpoint():
        return {"status": "ok"}
    
    # 检查系统内存
    memory_info = MemoryMonitor.check_system_memory()
    print(f"系统内存: {memory_info}")
