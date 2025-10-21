"""
优化的数据库连接池配置
"""
import aiomysql
import os
from typing import Any, Optional
from loguru import logger
import asyncio
from contextlib import asynccontextmanager


class DatabasePool:
    """
    优化的数据库连接池管理类
    
    特性:
    1. 连接池大小优化
    2. 连接健康检查
    3. 自动重连
    4. 连接池监控
    5. 优雅关闭
    """
    
    def __init__(
        self,
        host: str,
        port: int,
        user: str,
        password: str,
        db: str,
        minsize: int = 5,
        maxsize: int = 20,
        pool_recycle: int = 3600,
        connect_timeout: int = 10,
        charset: str = "utf8mb4"
    ):
        self.host = host
        self.port = port
        self.user = user
        self.password = password
        self.db = db
        self.minsize = minsize
        self.maxsize = maxsize
        self.pool_recycle = pool_recycle
        self.connect_timeout = connect_timeout
        self.charset = charset
        
        self._pool: Optional[aiomysql.Pool] = None
        self._lock = asyncio.Lock()
        
        # 监控指标
        self.total_connections_created = 0
        self.total_queries_executed = 0
        self.total_errors = 0
    
    async def get_pool(self) -> aiomysql.Pool:
        """
        获取连接池（懒加载）
        """
        if self._pool is None:
            async with self._lock:
                if self._pool is None:
                    try:
                        logger.info(
                            f"Creating database pool: {self.host}:{self.port}/{self.db}"
                        )
                        
                        self._pool = await aiomysql.create_pool(
                            host=self.host,
                            port=self.port,
                            user=self.user,
                            password=self.password,
                            db=self.db,
                            minsize=self.minsize,
                            maxsize=self.maxsize,
                            autocommit=False,  # 改为False，手动控制事务
                            charset=self.charset,
                            connect_timeout=self.connect_timeout,
                            pool_recycle=self.pool_recycle,
                            echo=False,  # 生产环境关闭SQL回显
                        )
                        
                        logger.info(
                            f"Database pool created successfully "
                            f"(minsize={self.minsize}, maxsize={self.maxsize})"
                        )
                        
                        # 测试连接
                        await self._test_connection()
                        
                    except Exception as e:
                        logger.error(f"Failed to create database pool: {e}")
                        raise
        
        return self._pool
    
    async def _test_connection(self):
        """测试数据库连接"""
        try:
            async with self._pool.acquire() as conn:
                async with conn.cursor() as cur:
                    await cur.execute("SELECT 1")
                    result = await cur.fetchone()
                    assert result == (1,), "Database connection test failed"
            logger.info("Database connection test passed")
        except Exception as e:
            logger.error(f"Database connection test failed: {e}")
            raise
    
    @asynccontextmanager
    async def acquire(self):
        """
        获取数据库连接的上下文管理器
        
        用法:
            async with db_pool.acquire() as conn:
                async with conn.cursor() as cur:
                    await cur.execute("SELECT * FROM users")
        """
        pool = await self.get_pool()
        conn = None
        try:
            conn = await pool.acquire()
            self.total_connections_created += 1
            yield conn
        finally:
            if conn:
                await pool.release(conn)
    
    async def execute_query(
        self,
        query: str,
        params: tuple = (),
        fetch_one: bool = False,
        fetch_all: bool = True
    ) -> Any:
        """
        执行查询并返回结果
        
        Args:
            query: SQL查询
            params: 查询参数
            fetch_one: 是否只获取一条记录
            fetch_all: 是否获取所有记录
        """
        import time
        start_time = time.time()
        
        try:
            async with self.acquire() as conn:
                async with conn.cursor(aiomysql.DictCursor) as cur:
                    await cur.execute(query, params)
                    
                    if fetch_one:
                        result = await cur.fetchone()
                    elif fetch_all:
                        result = await cur.fetchall()
                    else:
                        result = None
                    
                    self.total_queries_executed += 1
                    
                    # 记录慢查询
                    duration = time.time() - start_time
                    if duration > 1.0:
                        logger.warning(
                            f"Slow query detected: {duration:.3f}s",
                            extra={
                                "query": query[:200],  # 截断长查询
                                "duration": duration,
                                "params": str(params)[:100]
                            }
                        )
                    
                    return result
        
        except Exception as e:
            self.total_errors += 1
            logger.error(
                f"Query execution failed: {e}",
                extra={
                    "query": query[:200],
                    "params": str(params)[:100],
                    "error": str(e)
                }
            )
            raise
    
    async def execute_many(self, query: str, params: list[tuple]) -> None:
        """
        批量执行SQL（INSERT/UPDATE/DELETE）
        """
        try:
            async with self.acquire() as conn:
                async with conn.cursor() as cur:
                    await cur.executemany(query, params)
                    await conn.commit()
                    self.total_queries_executed += len(params)
        
        except Exception as e:
            self.total_errors += 1
            logger.error(f"Batch execution failed: {e}")
            raise
    
    async def close(self):
        """
        优雅关闭连接池
        """
        if self._pool:
            logger.info("Closing database pool...")
            self._pool.close()
            await self._pool.wait_closed()
            self._pool = None
            logger.info("Database pool closed")
    
    def get_stats(self) -> dict:
        """
        获取连接池统计信息
        """
        stats = {
            "total_connections_created": self.total_connections_created,
            "total_queries_executed": self.total_queries_executed,
            "total_errors": self.total_errors,
            "error_rate": (
                self.total_errors / self.total_queries_executed
                if self.total_queries_executed > 0 else 0
            ),
        }
        
        if self._pool:
            stats.update({
                "pool_size": self._pool.size,
                "pool_freesize": self._pool.freesize,
                "pool_minsize": self.minsize,
                "pool_maxsize": self.maxsize,
            })
        
        return stats
    
    async def health_check(self) -> dict:
        """
        数据库健康检查
        """
        try:
            import time
            start = time.time()
            
            async with self.acquire() as conn:
                async with conn.cursor() as cur:
                    await cur.execute("SELECT 1")
                    await cur.fetchone()
            
            ping_time = time.time() - start
            
            return {
                "status": "healthy",
                "ping_ms": round(ping_time * 1000, 2),
                "pool_stats": self.get_stats()
            }
        
        except Exception as e:
            return {
                "status": "unhealthy",
                "error": str(e),
                "pool_stats": self.get_stats()
            }


# ========== 全局连接池实例 ==========

_db_pool: Optional[DatabasePool] = None


async def init_database_pool(
    host: str = None,
    port: int = None,
    user: str = None,
    password: str = None,
    db: str = None,
    minsize: int = 5,
    maxsize: int = 20
) -> DatabasePool:
    """
    初始化全局数据库连接池
    
    从环境变量读取配置（如果未提供参数）
    """
    global _db_pool
    
    if _db_pool is None:
        # 从环境变量读取
        host = host or os.getenv("MYSQL_HOST", "127.0.0.1")
        port = port or int(os.getenv("MYSQL_PORT", "3306"))
        user = user or os.getenv("MYSQL_USER", "root")
        password = password or os.getenv("MYSQL_PASSWORD", "")
        db = db or os.getenv("MYSQL_DB", "fund_management")
        
        _db_pool = DatabasePool(
            host=host,
            port=port,
            user=user,
            password=password,
            db=db,
            minsize=minsize,
            maxsize=maxsize,
            pool_recycle=3600,  # 1小时回收连接
            connect_timeout=10,
        )
        
        # 立即创建连接池
        await _db_pool.get_pool()
    
    return _db_pool


def get_database_pool() -> DatabasePool:
    """
    获取全局数据库连接池
    """
    if _db_pool is None:
        raise RuntimeError(
            "Database pool not initialized. "
            "Call init_database_pool() first."
        )
    return _db_pool


async def close_database_pool():
    """
    关闭全局数据库连接池
    """
    global _db_pool
    if _db_pool:
        await _db_pool.close()
        _db_pool = None


# ========== 兼容旧代码的函数 ==========

async def get_pool() -> aiomysql.Pool:
    """兼容旧代码"""
    db_pool = get_database_pool()
    return await db_pool.get_pool()


async def close_pool() -> None:
    """兼容旧代码"""
    await close_database_pool()


async def execute_query(query: str, params: tuple = ()) -> Any:
    """兼容旧代码"""
    db_pool = get_database_pool()
    return await db_pool.execute_query(query, params)


async def execute_many(query: str, params: list[tuple]) -> None:
    """兼容旧代码"""
    db_pool = get_database_pool()
    await db_pool.execute_many(query, params)
