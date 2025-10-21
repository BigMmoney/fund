"""
数据库连接和会话管理 - 高性能、可靠的数据库访问层
支持连接池、事务管理、异步操作
"""
import logging
from contextlib import contextmanager, asynccontextmanager
from typing import Generator, AsyncGenerator, Optional
from sqlalchemy import create_engine, MetaData, event
from sqlalchemy.engine import Engine
from sqlalchemy.ext.declarative import declarative_base
from sqlalchemy.orm import sessionmaker, Session, scoped_session
from sqlalchemy.pool import QueuePool
from sqlalchemy.exc import SQLAlchemyError, DisconnectionError

from app.core.config import settings

logger = logging.getLogger(__name__)

# 声明性基础类
Base = declarative_base()

# 元数据 - 用于表结构管理
metadata = MetaData()

class DatabaseManager:
    """数据库管理器 - 单例模式"""
    
    _instance = None
    _engine: Optional[Engine] = None
    _session_factory: Optional[sessionmaker] = None
    _scoped_session_factory: Optional[scoped_session] = None
    
    def __new__(cls):
        if cls._instance is None:
            cls._instance = super().__new__(cls)
        return cls._instance
    
    def initialize(self):
        """初始化数据库连接"""
        if self._engine is not None:
            return
        
        logger.info("Initializing database connection...")
        
        # 创建数据库引擎
        self._engine = create_engine(
            settings.database.database_url,
            
            # 连接池配置
            poolclass=QueuePool,
            pool_size=settings.database.DB_POOL_SIZE,
            max_overflow=settings.database.DB_MAX_OVERFLOW,
            pool_timeout=settings.database.DB_POOL_TIMEOUT,
            pool_recycle=settings.database.DB_POOL_RECYCLE,
            pool_pre_ping=True,  # 连接健康检查
            
            # 引擎配置
            echo=settings.database.DB_ECHO,
            future=True,  # 使用SQLAlchemy 2.0 API
            
            # 连接参数
            connect_args={
                "charset": settings.database.DB_CHARSET,
                "autocommit": False,
                "check_same_thread": False  # SQLite兼容
            } if "sqlite" in settings.database.database_url else {
                "charset": settings.database.DB_CHARSET,
                "autocommit": False
            }
        )
        
        # 添加连接事件监听器
        self._setup_event_listeners()
        
        # 创建会话工厂
        self._session_factory = sessionmaker(
            bind=self._engine,
            autocommit=False,
            autoflush=False,
            expire_on_commit=False
        )
        
        # 创建作用域会话工厂
        self._scoped_session_factory = scoped_session(self._session_factory)
        
        logger.info("Database connection initialized successfully")
    
    def _setup_event_listeners(self):
        """设置数据库事件监听器"""
        
        @event.listens_for(self._engine, "connect")
        def set_sqlite_pragma(dbapi_connection, connection_record):
            """SQLite优化配置"""
            if "sqlite" in settings.database.database_url:
                cursor = dbapi_connection.cursor()
                cursor.execute("PRAGMA foreign_keys=ON")
                cursor.execute("PRAGMA journal_mode=WAL")
                cursor.execute("PRAGMA synchronous=NORMAL")
                cursor.execute("PRAGMA cache_size=1000")
                cursor.execute("PRAGMA temp_store=memory")
                cursor.close()
        
        @event.listens_for(self._engine, "checkout")
        def check_connection(dbapi_connection, connection_record, connection_proxy):
            """连接检出时的健康检查"""
            try:
                # 执行简单查询验证连接
                cursor = dbapi_connection.cursor()
                cursor.execute("SELECT 1")
                cursor.close()
            except Exception as e:
                logger.warning(f"Database connection check failed: {e}")
                # 让连接池处理无效连接
                raise DisconnectionError()
        
        @event.listens_for(self._engine, "invalid")
        def invalid_connection(dbapi_connection, connection_record, exception):
            """处理无效连接"""
            logger.error(f"Database connection invalidated: {exception}")
    
    @property
    def engine(self) -> Engine:
        """获取数据库引擎"""
        if self._engine is None:
            self.initialize()
        return self._engine
    
    @property
    def session_factory(self) -> sessionmaker:
        """获取会话工厂"""
        if self._session_factory is None:
            self.initialize()
        return self._session_factory
    
    @property
    def scoped_session(self) -> scoped_session:
        """获取作用域会话"""
        if self._scoped_session_factory is None:
            self.initialize()
        return self._scoped_session_factory
    
    def create_database(self):
        """创建数据库表"""
        logger.info("Creating database tables...")
        try:
            Base.metadata.create_all(bind=self.engine)
            logger.info("Database tables created successfully")
        except Exception as e:
            logger.error(f"Failed to create database tables: {e}")
            raise
    
    def drop_database(self):
        """删除数据库表 - 危险操作，仅用于开发环境"""
        if settings.app.is_production:
            raise RuntimeError("Cannot drop database in production environment")
        
        logger.warning("Dropping database tables...")
        try:
            Base.metadata.drop_all(bind=self.engine)
            logger.info("Database tables dropped successfully")
        except Exception as e:
            logger.error(f"Failed to drop database tables: {e}")
            raise
    
    def check_connection(self) -> bool:
        """检查数据库连接状态"""
        try:
            with self.session_factory() as session:
                session.execute("SELECT 1")
                return True
        except Exception as e:
            logger.error(f"Database connection check failed: {e}")
            return False
    
    def get_connection_info(self) -> dict:
        """获取连接信息"""
        pool = self.engine.pool
        return {
            "url": settings.database.database_url.split('@')[-1],  # 隐藏密码
            "pool_size": pool.size(),
            "checked_in": pool.checkedin(),
            "checked_out": pool.checkedout(),
            "overflow": pool.overflow(),
            "invalidated": pool.invalidated()
        }
    
    def close(self):
        """关闭数据库连接"""
        if self._scoped_session_factory:
            self._scoped_session_factory.remove()
        
        if self._engine:
            self._engine.dispose()
            logger.info("Database connections closed")
        
        self._engine = None
        self._session_factory = None
        self._scoped_session_factory = None


# 全局数据库管理器实例
db_manager = DatabaseManager()

def get_database_session() -> Generator[Session, None, None]:
    """获取数据库会话 - 用于依赖注入"""
    session = db_manager.session_factory()
    try:
        yield session
    except Exception as e:
        logger.error(f"Database session error: {e}")
        session.rollback()
        raise
    finally:
        session.close()

@contextmanager
def get_db_session() -> Generator[Session, None, None]:
    """获取数据库会话上下文管理器"""
    session = db_manager.session_factory()
    try:
        yield session
        session.commit()
    except Exception as e:
        logger.error(f"Database transaction error: {e}")
        session.rollback()
        raise
    finally:
        session.close()

class DatabaseTransaction:
    """数据库事务管理器"""
    
    def __init__(self, session: Session):
        self.session = session
        self._savepoint = None
    
    def __enter__(self):
        """进入事务"""
        if self.session.in_transaction():
            # 如果已经在事务中，创建保存点
            self._savepoint = self.session.begin_nested()
        else:
            # 开始新事务
            self.session.begin()
        return self
    
    def __exit__(self, exc_type, exc_val, exc_tb):
        """退出事务"""
        try:
            if exc_type is not None:
                # 有异常，回滚
                if self._savepoint:
                    self._savepoint.rollback()
                else:
                    self.session.rollback()
            else:
                # 无异常，提交
                if self._savepoint:
                    self._savepoint.commit()
                else:
                    self.session.commit()
        except Exception as e:
            logger.error(f"Transaction management error: {e}")
            if self._savepoint:
                self._savepoint.rollback()
            else:
                self.session.rollback()
            raise
    
    def commit(self):
        """手动提交事务"""
        if self._savepoint:
            self._savepoint.commit()
        else:
            self.session.commit()
    
    def rollback(self):
        """手动回滚事务"""
        if self._savepoint:
            self._savepoint.rollback()
        else:
            self.session.rollback()

def transaction(session: Session) -> DatabaseTransaction:
    """创建事务管理器"""
    return DatabaseTransaction(session)

class HealthCheck:
    """数据库健康检查"""
    
    @staticmethod
    def check_database() -> dict:
        """检查数据库状态"""
        try:
            # 检查连接
            connection_ok = db_manager.check_connection()
            
            # 获取连接池信息
            pool_info = db_manager.get_connection_info()
            
            # 检查表是否存在
            tables_exist = True
            try:
                with get_db_session() as session:
                    # 尝试查询一个系统表
                    session.execute("SELECT 1 FROM users LIMIT 1")
            except Exception:
                tables_exist = False
            
            return {
                "status": "healthy" if connection_ok and tables_exist else "unhealthy",
                "connection": connection_ok,
                "tables_exist": tables_exist,
                "pool_info": pool_info
            }
            
        except Exception as e:
            logger.error(f"Database health check failed: {e}")
            return {
                "status": "unhealthy",
                "error": str(e),
                "connection": False,
                "tables_exist": False
            }

# 初始化数据库
def init_database():
    """初始化数据库"""
    try:
        db_manager.initialize()
        
        # 在开发环境下创建表
        if settings.app.is_development:
            db_manager.create_database()
            
        logger.info("Database initialization completed")
        
    except Exception as e:
        logger.error(f"Database initialization failed: {e}")
        raise

# 清理资源
def cleanup_database():
    """清理数据库资源"""
    db_manager.close()

# 别名，保持向后兼容
get_db = get_database_session
engine = db_manager.engine