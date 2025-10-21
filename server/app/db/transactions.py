"""
数据库事务管理工具
提供事务装饰器、上下文管理器和乐观锁支持
"""
from functools import wraps
from contextlib import contextmanager
from typing import Callable, Generator, Optional
import time
import logging
from sqlalchemy.orm import Session
from sqlalchemy.exc import (
    SQLAlchemyError,
    OperationalError,
    IntegrityError,
    DatabaseError
)
from sqlalchemy import event, Column, Integer

logger = logging.getLogger(__name__)


class TransactionError(Exception):
    """事务相关错误"""
    pass


class DeadlockError(TransactionError):
    """死锁错误"""
    pass


class OptimisticLockError(TransactionError):
    """乐观锁冲突错误"""
    pass


def transactional(
    max_retries: int = 3,
    retry_delay: float = 0.5,
    retry_on_deadlock: bool = True,
    raise_on_error: bool = True
):
    """
    事务装饰器
    
    提供以下功能：
    1. 自动提交/回滚
    2. 死锁自动重试
    3. 异常处理和日志
    4. 事务执行时间监控
    
    Args:
        max_retries: 最大重试次数（用于死锁）
        retry_delay: 重试延迟（秒）
        retry_on_deadlock: 是否在死锁时自动重试
        raise_on_error: 是否在错误时抛出异常
    
    用法:
        @transactional(max_retries=3)
        async def my_function(db: Session, ...):
            # 业务逻辑
            pass
    """
    def decorator(func: Callable):
        @wraps(func)
        async def async_wrapper(*args, db: Session = None, **kwargs):
            if db is None:
                raise ValueError("transactional decorator requires 'db' parameter")
            
            retries = 0
            last_error = None
            start_time = time.time()
            
            while retries <= max_retries:
                try:
                    # 记录事务开始
                    logger.debug(
                        f"Transaction started: {func.__name__} (attempt {retries + 1}/{max_retries + 1})"
                    )
                    
                    # 执行函数
                    result = await func(*args, db=db, **kwargs)
                    
                    # 提交事务
                    db.commit()
                    
                    # 记录成功
                    duration = time.time() - start_time
                    logger.debug(
                        f"Transaction committed: {func.__name__} "
                        f"(duration: {duration:.3f}s, attempts: {retries + 1})"
                    )
                    
                    # 监控慢事务
                    if duration > 5.0:
                        logger.warning(
                            f"Slow transaction detected: {func.__name__} took {duration:.3f}s",
                            extra={
                                "function": func.__name__,
                                "duration": duration,
                                "attempts": retries + 1
                            }
                        )
                    
                    return result
                
                except OperationalError as e:
                    # 数据库操作错误（可能是死锁）
                    db.rollback()
                    last_error = e
                    
                    error_str = str(e).lower()
                    is_deadlock = any(keyword in error_str for keyword in [
                        "deadlock", "lock wait timeout", "database is locked"
                    ])
                    
                    if is_deadlock and retry_on_deadlock and retries < max_retries:
                        retries += 1
                        sleep_time = retry_delay * retries  # 指数退避
                        
                        logger.warning(
                            f"Deadlock detected in {func.__name__}, "
                            f"retrying in {sleep_time:.2f}s (attempt {retries + 1}/{max_retries + 1})",
                            extra={
                                "function": func.__name__,
                                "error": str(e),
                                "retry_attempt": retries
                            }
                        )
                        
                        time.sleep(sleep_time)
                        continue
                    else:
                        # 不是死锁或已达到最大重试次数
                        logger.error(
                            f"Database operational error in {func.__name__}: {e}",
                            extra={
                                "function": func.__name__,
                                "error": str(e),
                                "is_deadlock": is_deadlock,
                                "retries_exhausted": retries >= max_retries
                            }
                        )
                        if raise_on_error:
                            if is_deadlock:
                                raise DeadlockError(
                                    f"Transaction failed after {retries + 1} attempts due to deadlock"
                                ) from e
                            raise TransactionError(f"Database operation failed: {e}") from e
                        break
                
                except IntegrityError as e:
                    # 完整性约束错误（唯一键冲突、外键约束等）
                    db.rollback()
                    logger.error(
                        f"Integrity constraint violation in {func.__name__}: {e}",
                        extra={
                            "function": func.__name__,
                            "error": str(e)
                        }
                    )
                    if raise_on_error:
                        raise TransactionError(f"Data integrity violation: {e}") from e
                    break
                
                except SQLAlchemyError as e:
                    # 其他SQLAlchemy错误
                    db.rollback()
                    logger.error(
                        f"Database error in {func.__name__}: {e}",
                        extra={
                            "function": func.__name__,
                            "error": str(e),
                            "error_type": type(e).__name__
                        }
                    )
                    if raise_on_error:
                        raise TransactionError(f"Database error: {e}") from e
                    break
                
                except Exception as e:
                    # 非数据库错误也要回滚
                    db.rollback()
                    logger.error(
                        f"Unexpected error in {func.__name__}: {e}",
                        extra={
                            "function": func.__name__,
                            "error": str(e),
                            "error_type": type(e).__name__
                        },
                        exc_info=True
                    )
                    if raise_on_error:
                        raise
                    break
            
            # 如果所有重试都失败了
            if last_error and raise_on_error:
                raise TransactionError(
                    f"Transaction failed after {retries + 1} attempts"
                ) from last_error
            
            return None
        
        @wraps(func)
        def sync_wrapper(*args, db: Session = None, **kwargs):
            """同步函数的包装器"""
            if db is None:
                raise ValueError("transactional decorator requires 'db' parameter")
            
            retries = 0
            last_error = None
            start_time = time.time()
            
            while retries <= max_retries:
                try:
                    result = func(*args, db=db, **kwargs)
                    db.commit()
                    
                    duration = time.time() - start_time
                    logger.debug(
                        f"Transaction committed: {func.__name__} (duration: {duration:.3f}s)"
                    )
                    
                    return result
                
                except OperationalError as e:
                    db.rollback()
                    last_error = e
                    
                    error_str = str(e).lower()
                    is_deadlock = any(keyword in error_str for keyword in [
                        "deadlock", "lock wait timeout"
                    ])
                    
                    if is_deadlock and retry_on_deadlock and retries < max_retries:
                        retries += 1
                        time.sleep(retry_delay * retries)
                        continue
                    else:
                        if raise_on_error:
                            raise TransactionError(f"Database operation failed: {e}") from e
                        break
                
                except (IntegrityError, SQLAlchemyError) as e:
                    db.rollback()
                    logger.error(f"Database error in {func.__name__}: {e}")
                    if raise_on_error:
                        raise TransactionError(f"Database error: {e}") from e
                    break
                
                except Exception as e:
                    db.rollback()
                    logger.error(f"Unexpected error in {func.__name__}: {e}", exc_info=True)
                    if raise_on_error:
                        raise
                    break
            
            return None
        
        # 根据函数类型返回相应的包装器
        import asyncio
        if asyncio.iscoroutinefunction(func):
            return async_wrapper
        else:
            return sync_wrapper
    
    return decorator


@contextmanager
def transaction_scope(session_factory) -> Generator[Session, None, None]:
    """
    事务上下文管理器
    
    用法:
        with transaction_scope(SessionLocal) as db:
            # 业务逻辑
            db.add(new_object)
            # 自动提交或回滚
    """
    session = session_factory()
    try:
        yield session
        session.commit()
        logger.debug("Transaction committed via context manager")
    except Exception as e:
        session.rollback()
        logger.error(f"Transaction rolled back: {e}", exc_info=True)
        raise
    finally:
        session.close()


class OptimisticLockMixin:
    """
    乐观锁Mixin - 添加到需要版本控制的模型中
    
    用法:
        class MyModel(Base, OptimisticLockMixin):
            __tablename__ = "my_table"
            id = Column(Integer, primary_key=True)
            # ... 其他字段
    """
    version = Column(Integer, default=1, nullable=False)
    
    __mapper_args__ = {
        "version_id_col": "version"
    }


def with_row_lock(query, lock_mode: str = "update"):
    """
    为查询添加行锁
    
    Args:
        query: SQLAlchemy查询对象
        lock_mode: 锁模式 - "update" (FOR UPDATE) 或 "share" (FOR SHARE)
    
    用法:
        user = with_row_lock(
            db.query(User).filter(User.id == user_id),
            lock_mode="update"
        ).first()
    """
    if lock_mode == "update":
        return query.with_for_update()
    elif lock_mode == "share":
        return query.with_for_update(read=True)
    else:
        raise ValueError(f"Invalid lock_mode: {lock_mode}")


# ========== 事务监控 ==========

class TransactionMonitor:
    """事务性能监控"""
    
    def __init__(self):
        self.transaction_count = 0
        self.rollback_count = 0
        self.commit_count = 0
        self.total_duration = 0.0
    
    def on_transaction_start(self):
        self.transaction_count += 1
    
    def on_commit(self, duration: float):
        self.commit_count += 1
        self.total_duration += duration
    
    def on_rollback(self):
        self.rollback_count += 1
    
    def get_stats(self) -> dict:
        """获取统计信息"""
        return {
            "total_transactions": self.transaction_count,
            "commits": self.commit_count,
            "rollbacks": self.rollback_count,
            "rollback_rate": (
                self.rollback_count / self.transaction_count 
                if self.transaction_count > 0 else 0
            ),
            "avg_duration": (
                self.total_duration / self.commit_count
                if self.commit_count > 0 else 0
            )
        }


# 全局监控实例
transaction_monitor = TransactionMonitor()


def setup_transaction_monitoring(engine):
    """
    为数据库引擎设置事务监控
    
    用法:
        from app.db.mysql import engine
        setup_transaction_monitoring(engine)
    """
    @event.listens_for(engine, "begin")
    def receive_begin(conn):
        transaction_monitor.on_transaction_start()
    
    @event.listens_for(engine, "commit")
    def receive_commit(conn):
        # 注意：这里无法直接获取持续时间，需要在应用层计算
        transaction_monitor.on_commit(0)
    
    @event.listens_for(engine, "rollback")
    def receive_rollback(conn):
        transaction_monitor.on_rollback()
    
    logger.info("Transaction monitoring enabled")
