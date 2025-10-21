"""
内存监控工具 - 防止OOM
提供内存使用监控、告警和自动清理功能
"""
import os
import gc
import logging
import psutil
from typing import Dict, Any
from datetime import datetime

logger = logging.getLogger(__name__)


class MemoryMonitor:
    """内存监控器"""
    
    def __init__(self, warning_threshold: float = 70.0, critical_threshold: float = 85.0):
        """
        初始化内存监控器
        
        Args:
            warning_threshold: 内存使用警告阈值（百分比）
            critical_threshold: 内存使用严重阈值（百分比）
        """
        self.warning_threshold = warning_threshold
        self.critical_threshold = critical_threshold
        self.process = psutil.Process(os.getpid())
        self.baseline_memory = None
    
    def get_memory_info(self) -> Dict[str, Any]:
        """
        获取当前内存使用信息
        
        Returns:
            内存信息字典
        """
        try:
            mem_info = self.process.memory_info()
            memory_percent = self.process.memory_percent()
            
            # 系统内存信息
            system_memory = psutil.virtual_memory()
            
            info = {
                # 进程内存使用
                'process': {
                    'rss_mb': round(mem_info.rss / 1024 / 1024, 2),  # 物理内存
                    'vms_mb': round(mem_info.vms / 1024 / 1024, 2),  # 虚拟内存
                    'percent': round(memory_percent, 2),
                    'num_threads': self.process.num_threads(),
                },
                # 系统内存信息
                'system': {
                    'total_mb': round(system_memory.total / 1024 / 1024, 2),
                    'available_mb': round(system_memory.available / 1024 / 1024, 2),
                    'used_mb': round(system_memory.used / 1024 / 1024, 2),
                    'percent': round(system_memory.percent, 2),
                },
                # 状态
                'status': self._get_status(memory_percent),
                'timestamp': datetime.now().isoformat()
            }
            
            # 如果有基准，计算增长
            if self.baseline_memory:
                growth = mem_info.rss - self.baseline_memory
                info['growth_mb'] = round(growth / 1024 / 1024, 2)
            
            return info
            
        except Exception as e:
            logger.error(f"Failed to get memory info: {e}")
            return {
                'error': str(e),
                'status': 'error'
            }
    
    def _get_status(self, memory_percent: float) -> str:
        """根据内存使用率获取状态"""
        if memory_percent >= self.critical_threshold:
            return 'critical'
        elif memory_percent >= self.warning_threshold:
            return 'warning'
        else:
            return 'healthy'
    
    def check_and_alert(self) -> Dict[str, Any]:
        """
        检查内存使用并发出告警
        
        Returns:
            检查结果和告警信息
        """
        info = self.get_memory_info()
        status = info.get('status', 'unknown')
        memory_percent = info.get('process', {}).get('percent', 0)
        
        if status == 'critical':
            logger.error(
                f"🔴 CRITICAL: Memory usage at {memory_percent:.1f}% "
                f"(threshold: {self.critical_threshold}%)"
            )
            # 触发强制垃圾回收
            self.force_cleanup()
            
        elif status == 'warning':
            logger.warning(
                f"🟡 WARNING: Memory usage at {memory_percent:.1f}% "
                f"(threshold: {self.warning_threshold}%)"
            )
        
        return info
    
    def set_baseline(self):
        """设置内存基准线"""
        mem_info = self.process.memory_info()
        self.baseline_memory = mem_info.rss
        logger.info(f"Memory baseline set: {self.baseline_memory / 1024 / 1024:.2f} MB")
    
    def force_cleanup(self) -> Dict[str, Any]:
        """
        强制内存清理
        
        Returns:
            清理前后的内存对比
        """
        before = self.get_memory_info()
        
        logger.info("Starting forced memory cleanup...")
        
        # 触发垃圾回收
        collected = gc.collect()
        
        # 再次收集未引用的对象
        collected += gc.collect(generation=1)
        collected += gc.collect(generation=2)
        
        after = self.get_memory_info()
        
        before_mb = before.get('process', {}).get('rss_mb', 0)
        after_mb = after.get('process', {}).get('rss_mb', 0)
        freed_mb = before_mb - after_mb
        
        logger.info(
            f"Memory cleanup completed: "
            f"collected {collected} objects, "
            f"freed {freed_mb:.2f} MB"
        )
        
        return {
            'before': before,
            'after': after,
            'objects_collected': collected,
            'memory_freed_mb': round(freed_mb, 2)
        }
    
    def get_gc_stats(self) -> Dict[str, Any]:
        """
        获取垃圾回收统计信息
        
        Returns:
            GC统计信息
        """
        return {
            'counts': gc.get_count(),  # (gen0, gen1, gen2)
            'threshold': gc.get_threshold(),
            'garbage_objects': len(gc.garbage),
            'stats': gc.get_stats()
        }


# 全局内存监控实例
memory_monitor = MemoryMonitor(
    warning_threshold=70.0,   # 70%时警告
    critical_threshold=85.0   # 85%时严重告警
)


def get_memory_status() -> Dict[str, Any]:
    """获取内存状态（便捷函数）"""
    return memory_monitor.get_memory_info()


def check_memory() -> Dict[str, Any]:
    """检查内存并告警（便捷函数）"""
    return memory_monitor.check_and_alert()


def cleanup_memory() -> Dict[str, Any]:
    """强制清理内存（便捷函数）"""
    return memory_monitor.force_cleanup()


# 自动内存监控装饰器
def with_memory_cleanup(func):
    """
    装饰器：在函数执行后自动清理内存
    适用于大数据处理函数
    
    使用示例:
        @with_memory_cleanup
        async def process_large_data():
            # 处理大量数据
            pass
    """
    import functools
    
    @functools.wraps(func)
    async def wrapper(*args, **kwargs):
        # 执行函数
        result = await func(*args, **kwargs)
        
        # 执行后清理
        gc.collect()
        
        return result
    
    return wrapper
