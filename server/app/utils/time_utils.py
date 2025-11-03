"""
时间戳工具函数
所有时间使用Unix时间戳(整数)，避免时区问题
"""
import time
from datetime import datetime
from typing import Optional


def get_current_timestamp() -> int:
    """
    获取当前Unix时间戳(秒)
    
    Returns:
        int: 当前时间的Unix时间戳
    """
    return int(time.time())


def timestamp_to_datetime(timestamp: Optional[int]) -> Optional[datetime]:
    """
    将Unix时间戳转换为datetime对象
    
    Args:
        timestamp: Unix时间戳(秒)
        
    Returns:
        datetime对象，如果timestamp为None则返回None
    """
    if timestamp is None:
        return None
    return datetime.fromtimestamp(timestamp)


def datetime_to_timestamp(dt: Optional[datetime]) -> Optional[int]:
    """
    将datetime对象转换为Unix时间戳
    
    Args:
        dt: datetime对象
        
    Returns:
        Unix时间戳(秒)，如果dt为None则返回None
    """
    if dt is None:
        return None
    return int(dt.timestamp())


def format_timestamp(timestamp: Optional[int], format_str: str = "%Y-%m-%d %H:%M:%S") -> Optional[str]:
    """
    格式化Unix时间戳为字符串
    
    Args:
        timestamp: Unix时间戳(秒)
        format_str: 格式化字符串
        
    Returns:
        格式化后的时间字符串，如果timestamp为None则返回None
    """
    if timestamp is None:
        return None
    dt = timestamp_to_datetime(timestamp)
    return dt.strftime(format_str) if dt else None
