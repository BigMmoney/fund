"""
Redis缓存层实现
提供API响应缓存、数据缓存、分布式锁等功能
"""
import json
import hashlib
from typing import Optional, Any, Callable
from functools import wraps
from datetime import timedelta
import redis.asyncio as redis
from loguru import logger

from server.app.settings import settings


class CacheManager:
    """Redis缓存管理器"""
    
    def __init__(self):
        self.redis_client: Optional[redis.Redis] = None
        self.enabled = getattr(settings, 'CACHE_ENABLED', True)
        self.default_ttl = getattr(settings, 'CACHE_DEFAULT_TTL', 300)  # 5分钟
        
    async def connect(self):
        """连接到Redis"""
        if not self.enabled:
            logger.warning("Cache is disabled")
            return
            
        try:
            self.redis_client = redis.from_url(
                settings.REDIS_URL,
                encoding="utf-8",
                decode_responses=True,
                socket_connect_timeout=5,
                socket_keepalive=True,
                health_check_interval=30
            )
            # 测试连接
            await self.redis_client.ping()
            logger.info(f"Connected to Redis: {settings.REDIS_URL}")
        except Exception as e:
            logger.error(f"Failed to connect to Redis: {e}")
            self.enabled = False
            self.redis_client = None
    
    async def disconnect(self):
        """断开Redis连接"""
        if self.redis_client:
            await self.redis_client.close()
            logger.info("Disconnected from Redis")
    
    async def get(self, key: str) -> Optional[Any]:
        """获取缓存值"""
        if not self.enabled or not self.redis_client:
            return None
        
        try:
            value = await self.redis_client.get(key)
            if value:
                logger.debug(f"Cache hit: {key}")
                return json.loads(value)
            logger.debug(f"Cache miss: {key}")
            return None
        except Exception as e:
            logger.error(f"Cache get error for key {key}: {e}")
            return None
    
    async def set(
        self,
        key: str,
        value: Any,
        ttl: Optional[int] = None
    ) -> bool:
        """设置缓存值"""
        if not self.enabled or not self.redis_client:
            return False
        
        try:
            ttl = ttl or self.default_ttl
            serialized = json.dumps(value, ensure_ascii=False)
            await self.redis_client.setex(key, ttl, serialized)
            logger.debug(f"Cache set: {key} (TTL: {ttl}s)")
            return True
        except Exception as e:
            logger.error(f"Cache set error for key {key}: {e}")
            return False
    
    async def delete(self, key: str) -> bool:
        """删除缓存"""
        if not self.enabled or not self.redis_client:
            return False
        
        try:
            await self.redis_client.delete(key)
            logger.debug(f"Cache deleted: {key}")
            return True
        except Exception as e:
            logger.error(f"Cache delete error for key {key}: {e}")
            return False
    
    async def delete_pattern(self, pattern: str) -> int:
        """删除匹配模式的所有缓存"""
        if not self.enabled or not self.redis_client:
            return 0
        
        try:
            keys = []
            async for key in self.redis_client.scan_iter(match=pattern):
                keys.append(key)
            
            if keys:
                deleted = await self.redis_client.delete(*keys)
                logger.info(f"Deleted {deleted} cache keys matching: {pattern}")
                return deleted
            return 0
        except Exception as e:
            logger.error(f"Cache delete pattern error for {pattern}: {e}")
            return 0
    
    async def exists(self, key: str) -> bool:
        """检查缓存是否存在"""
        if not self.enabled or not self.redis_client:
            return False
        
        try:
            return await self.redis_client.exists(key) > 0
        except Exception as e:
            logger.error(f"Cache exists error for key {key}: {e}")
            return False
    
    async def ttl(self, key: str) -> int:
        """获取缓存剩余生存时间"""
        if not self.enabled or not self.redis_client:
            return -1
        
        try:
            return await self.redis_client.ttl(key)
        except Exception as e:
            logger.error(f"Cache TTL error for key {key}: {e}")
            return -1
    
    async def incr(self, key: str, amount: int = 1) -> Optional[int]:
        """递增计数器"""
        if not self.enabled or not self.redis_client:
            return None
        
        try:
            return await self.redis_client.incrby(key, amount)
        except Exception as e:
            logger.error(f"Cache incr error for key {key}: {e}")
            return None
    
    async def expire(self, key: str, ttl: int) -> bool:
        """设置过期时间"""
        if not self.enabled or not self.redis_client:
            return False
        
        try:
            return await self.redis_client.expire(key, ttl)
        except Exception as e:
            logger.error(f"Cache expire error for key {key}: {e}")
            return False


# 全局缓存管理器实例
cache_manager = CacheManager()


def cache_key(*args, prefix: str = "api", **kwargs) -> str:
    """生成缓存键"""
    # 将所有参数转换为字符串并排序
    parts = [prefix]
    
    # 添加位置参数
    for arg in args:
        parts.append(str(arg))
    
    # 添加关键字参数（排序以保证一致性）
    for k, v in sorted(kwargs.items()):
        parts.append(f"{k}={v}")
    
    # 如果键太长，使用MD5哈希
    key = ":".join(parts)
    if len(key) > 200:
        key_hash = hashlib.md5(key.encode()).hexdigest()
        key = f"{prefix}:hash:{key_hash}"
    
    return key


def cached(
    ttl: Optional[int] = None,
    prefix: str = "api",
    key_builder: Optional[Callable] = None
):
    """
    缓存装饰器
    
    用法:
        @cached(ttl=300, prefix="user")
        async def get_user(user_id: int):
            return await db.fetch_user(user_id)
    """
    def decorator(func: Callable):
        @wraps(func)
        async def wrapper(*args, **kwargs):
            # 生成缓存键
            if key_builder:
                key = key_builder(*args, **kwargs)
            else:
                # 排除self/cls参数
                cache_args = args[1:] if args and hasattr(args[0], '__class__') else args
                key = cache_key(*cache_args, prefix=prefix, **kwargs)
            
            # 尝试从缓存获取
            cached_value = await cache_manager.get(key)
            if cached_value is not None:
                logger.debug(f"Cache hit for {func.__name__}: {key}")
                return cached_value
            
            # 调用原函数
            result = await func(*args, **kwargs)
            
            # 存入缓存
            if result is not None:
                await cache_manager.set(key, result, ttl)
                logger.debug(f"Cached result for {func.__name__}: {key}")
            
            return result
        
        return wrapper
    return decorator


class DistributedLock:
    """分布式锁"""
    
    def __init__(
        self,
        key: str,
        timeout: int = 10,
        retry_times: int = 3,
        retry_delay: float = 0.1
    ):
        self.key = f"lock:{key}"
        self.timeout = timeout
        self.retry_times = retry_times
        self.retry_delay = retry_delay
        self.lock_value = None
    
    async def __aenter__(self):
        """获取锁"""
        import asyncio
        import uuid
        
        self.lock_value = str(uuid.uuid4())
        
        for i in range(self.retry_times):
            acquired = await cache_manager.redis_client.set(
                self.key,
                self.lock_value,
                nx=True,  # 只在键不存在时设置
                ex=self.timeout
            )
            
            if acquired:
                logger.debug(f"Acquired lock: {self.key}")
                return self
            
            if i < self.retry_times - 1:
                await asyncio.sleep(self.retry_delay)
        
        raise TimeoutError(f"Failed to acquire lock: {self.key}")
    
    async def __aexit__(self, exc_type, exc_val, exc_tb):
        """释放锁"""
        if not self.lock_value:
            return
        
        # 使用Lua脚本确保只删除自己的锁
        lua_script = """
        if redis.call("get", KEYS[1]) == ARGV[1] then
            return redis.call("del", KEYS[1])
        else
            return 0
        end
        """
        
        try:
            await cache_manager.redis_client.eval(
                lua_script,
                1,
                self.key,
                self.lock_value
            )
            logger.debug(f"Released lock: {self.key}")
        except Exception as e:
            logger.error(f"Failed to release lock {self.key}: {e}")


async def invalidate_cache(patterns: list[str]):
    """批量清除缓存"""
    total_deleted = 0
    for pattern in patterns:
        deleted = await cache_manager.delete_pattern(pattern)
        total_deleted += deleted
    
    logger.info(f"Invalidated {total_deleted} cache entries")
    return total_deleted


# ========== 预定义的缓存模式 ==========

class CachePatterns:
    """常用的缓存键模式"""
    
    # API响应缓存
    API_RESPONSE = "api:response:*"
    
    # 用户相关
    USER = "user:*"
    USER_BY_ID = "user:id:*"
    USER_BY_EMAIL = "user:email:*"
    
    # 子账户相关
    SUBACCOUNT = "subaccount:*"
    SUBACCOUNT_BY_ID = "subaccount:id:*"
    SUBACCOUNT_LIST = "subaccount:list:*"
    
    # 持仓相关
    POSITION = "position:*"
    POSITION_BY_ACCOUNT = "position:account:*"
    
    # 交易相关
    TRADE = "trade:*"
    TRADE_HISTORY = "trade:history:*"
    
    # 统计数据
    STATS = "stats:*"
    METRICS = "metrics:*"


# ========== 辅助函数 ==========

async def cache_warmup():
    """缓存预热 - 在应用启动时加载常用数据"""
    logger.info("Starting cache warmup...")
    
    # TODO: 根据实际业务需求预加载数据
    # 例如：
    # - 加载活跃用户列表
    # - 加载配置数据
    # - 加载统计数据
    
    logger.info("Cache warmup completed")


async def cache_stats() -> dict:
    """获取缓存统计信息"""
    if not cache_manager.enabled or not cache_manager.redis_client:
        return {"enabled": False}
    
    try:
        info = await cache_manager.redis_client.info()
        return {
            "enabled": True,
            "connected_clients": info.get("connected_clients"),
            "used_memory": info.get("used_memory_human"),
            "total_keys": await cache_manager.redis_client.dbsize(),
            "hit_rate": info.get("keyspace_hits", 0) / max(
                info.get("keyspace_hits", 0) + info.get("keyspace_misses", 0), 1
            )
        }
    except Exception as e:
        logger.error(f"Failed to get cache stats: {e}")
        return {"enabled": True, "error": str(e)}
