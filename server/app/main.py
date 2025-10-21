"""
Main FastAPI application
"""
from contextlib import asynccontextmanager
from fastapi import FastAPI, HTTPException
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import JSONResponse
import logging
import uvicorn

from server.app.config import settings
from server.app.database import engine, Base
from server.app.api.routers import (
    users, health, portfolios, teams, snapshots, ceffu
)
from server.app.models import Permission
from server.app.services.scheduler import start_scheduler, stop_scheduler

# P0改进：导入新增模块
from server.app.logging_config import setup_logging
from server.app.middlewares import (
    RequestTracingMiddleware,
    PerformanceMonitoringMiddleware,
    SecurityHeadersMiddleware
)
from server.app.exception_handlers import register_exception_handlers
from server.app.rate_limit import setup_rate_limiting

# P1功能：导入缓存、任务、监控模块
try:
    from server.app.cache import CacheManager, get_cache_manager
    from server.app.monitoring import MonitoringManager, PrometheusMiddleware
    from server.app.tasks import celery_app
    P1_FEATURES_AVAILABLE = True
    logger_p1 = logging.getLogger(__name__)
    logger_p1.info("✅ P1功能模块加载成功 (cache, monitoring, tasks)")
except ImportError as e:
    P1_FEATURES_AVAILABLE = False
    logger_p1 = logging.getLogger(__name__)
    logger_p1.warning(f"⚠️  P1功能模块加载失败: {e}")

# P0改进：初始化日志系统
setup_logging(
    log_level=settings.log_level if hasattr(settings, 'log_level') else "INFO",
    log_file_path="logs/app.log",
    environment="production" if hasattr(settings, 'database_url') and 'rds.amazonaws.com' in settings.database_url else "development"
)

# Configure logging
logging.basicConfig(
    level=logging.INFO,
    format='%(asctime)s - %(name)s - %(levelname)s - %(message)s'
)
logger = logging.getLogger(__name__)


@asynccontextmanager
async def lifespan(app: FastAPI):
    """Application lifespan events"""
    # Startup
    logger.info("Starting application...")
    
    # P1功能：初始化缓存管理器
    cache_mgr = None
    monitoring_mgr = None
    if P1_FEATURES_AVAILABLE:
        try:
            logger.info("🔧 初始化P1功能...")
            
            # 初始化缓存
            cache_mgr = get_cache_manager()
            await cache_mgr.initialize()
            logger.info("✅ Redis缓存管理器启动成功")
            
            # 初始化监控
            monitoring_mgr = MonitoringManager()
            await monitoring_mgr.start()
            logger.info("✅ Prometheus监控管理器启动成功")
            
            # Celery信息
            logger.info(f"✅ Celery任务队列已配置 ({len(celery_app.tasks)} 个任务)")
            
        except Exception as e:
            logger.warning(f"⚠️  P1功能初始化失败: {e}")
    
    # Create database tables
    try:
        Base.metadata.create_all(bind=engine)
        logger.info("Database tables created successfully")
        
        # Initialize default permissions
        await initialize_permissions()
        
        # Start snapshot scheduler
        await start_scheduler()
        logger.info("Snapshot scheduler started")
        
    except Exception as e:
        logger.error(f"Failed to create database tables: {e}")
        raise
    
    yield
    
    # Shutdown
    logger.info("Shutting down application...")
    
    # P1功能：关闭缓存和监控
    if P1_FEATURES_AVAILABLE:
        try:
            if cache_mgr:
                await cache_mgr.close()
                logger.info("✅ 缓存管理器已关闭")
            if monitoring_mgr:
                await monitoring_mgr.stop()
                logger.info("✅ 监控管理器已关闭")
        except Exception as e:
            logger.error(f"❌ P1功能关闭失败: {e}")
    
    await stop_scheduler()
    logger.info("Snapshot scheduler stopped")


async def initialize_permissions():
    """Initialize default permissions in database"""
    from database import SessionLocal
    
    db = SessionLocal()
    try:
        # Check if permissions already exist
        existing_permissions = db.query(Permission).count()
        if existing_permissions > 0:
            logger.info("Permissions already initialized")
            return
        
        # Create default permissions
        default_permissions = [
            Permission(
                id="user",
                label="User Management",
                description="Manage users and their accounts"
            ),
            Permission(
                id="team",
                label="Team Management", 
                description="Manage teams and team memberships"
            ),
            Permission(
                id="profit",
                label="Profit Management",
                description="Manage profit allocation and distribution"
            ),
            Permission(
                id="portfolio",
                label="Portfolio Management",
                description="Manage portfolios and investments"
            ),
            Permission(
                id="blacklist",
                label="Blacklist Management",
                description="Manage wallet blacklist and security"
            )
        ]
        
        for permission in default_permissions:
            db.add(permission)
        
        db.commit()
        logger.info("Default permissions created successfully")
        
    except Exception as e:
        logger.error(f"Failed to initialize permissions: {e}")
        db.rollback()
    finally:
        db.close()


# Create FastAPI app
app = FastAPI(
    title="Fund Management API",
    description="Comprehensive fund management system with portfolio tracking and profit distribution",
    version="1.0.0",
    docs_url="/docs",
    redoc_url="/redoc",
    lifespan=lifespan
)

# P0改进：注册中间件（顺序很重要！）
# 1. 安全头
app.add_middleware(SecurityHeadersMiddleware)

# 2. 请求追踪（生成trace_id）
app.add_middleware(RequestTracingMiddleware)

# 3. 性能监控
app.add_middleware(PerformanceMonitoringMiddleware)

# 4. P1功能：Prometheus监控中间件
if P1_FEATURES_AVAILABLE:
    try:
        app.add_middleware(PrometheusMiddleware)
        logger.info("✅ Prometheus监控中间件已启用")
    except Exception as e:
        logger.warning(f"⚠️  Prometheus中间件加载失败: {e}")

# 5. CORS（最后）
app.add_middleware(
    CORSMiddleware,
    allow_origins=[
        "http://13.113.11.170",
        "https://fund-api.cedefi.com",
        "http://fund-api.cedefi.com",
        "http://localhost:3000",  # Development frontend
        "http://localhost:8080",  # Development frontend
    ],
    allow_credentials=True,
    allow_methods=["GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"],
    allow_headers=["*"],
)

# P0改进：注册异常处理器
register_exception_handlers(app)

# P0改进：设置API限流
try:
    setup_rate_limiting(
        app,
        redis_url=settings.redis_url if hasattr(settings, 'redis_url') else None
    )
    logger.info("✅ API rate limiting enabled")
except Exception as e:
    logger.warning(f"⚠️  Rate limiting setup failed (will continue without it): {e}")


# Include routers - 完整的40个API接口
app.include_router(health.router)

# Import and include all other routers
from app.api.routers import (
    auth, profits, blacklist, profit_analytics, flows, subaccounts, 
    system, frontend_data, nav_ws, portfolio_nav, onetoken_api,
    onetoken_standard_api, account_web, r
)

app.include_router(auth.router)
app.include_router(users.router)
app.include_router(portfolios.router)
app.include_router(teams.router)
app.include_router(snapshots.router)
app.include_router(ceffu.router)

# Import allocation ratios router
from app.api import allocation_ratios
app.include_router(allocation_ratios.router, prefix="/api/v1")

app.include_router(profits.router)
app.include_router(blacklist.router)
app.include_router(profit_analytics.router)
app.include_router(flows.router)
app.include_router(subaccounts.router)
app.include_router(system.router)
app.include_router(frontend_data.router)
app.include_router(nav_ws.router)
app.include_router(portfolio_nav.router)
app.include_router(onetoken_api.router)
app.include_router(onetoken_standard_api.router)
app.include_router(account_web.router)
app.include_router(r.router)


@app.get("/")
async def root():
    """Root endpoint"""
    return {
        "message": "Fund Management API",
        "version": "1.0.0",
        "docs": "/docs",
        "features": {
            "p0_complete": True,
            "p1_enabled": P1_FEATURES_AVAILABLE,
            "p1_features": ["cache", "monitoring", "tasks"] if P1_FEATURES_AVAILABLE else []
        }
    }


# P1功能：添加Prometheus指标端点
if P1_FEATURES_AVAILABLE:
    from fastapi import Response
    from prometheus_client import generate_latest, CONTENT_TYPE_LATEST
    
    @app.get("/metrics")
    async def metrics():
        """Prometheus metrics endpoint"""
        return Response(content=generate_latest(), media_type=CONTENT_TYPE_LATEST)
    
    @app.get("/api/monitoring/health")
    async def monitoring_health():
        """监控系统健康检查"""
        try:
            monitoring_mgr = MonitoringManager()
            health_status = await monitoring_mgr.check_health()
            return {
                "status": "healthy" if all(v.get("healthy", False) for v in health_status.values()) else "unhealthy",
                "checks": health_status
            }
        except Exception as e:
            return {
                "status": "error",
                "error": str(e)
            }
    
    logger.info("✅ P1监控端点已注册: /metrics, /api/monitoring/health")


if __name__ == "__main__":
    uvicorn.run(
        "main:app",
        host="0.0.0.0",
        port=8001,
        reload=False
    )