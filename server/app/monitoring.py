"""
Prometheus监控和告警集成
提供性能指标收集、健康检查、告警等功能
"""
from prometheus_client import (
    Counter, Histogram, Gauge, Info,
    generate_latest, CONTENT_TYPE_LATEST,
    CollectorRegistry, multiprocess, push_to_gateway
)
from prometheus_client.exposition import generate_latest
from starlette.middleware.base import BaseHTTPMiddleware
from starlette.requests import Request
from starlette.responses import Response
from loguru import logger
from datetime import datetime
from typing import Optional, Dict, Any
import time
import psutil
import asyncio

from server.app.settings import settings


# ========== Prometheus指标定义 ==========

# 创建注册表
registry = CollectorRegistry()

# HTTP请求指标
http_requests_total = Counter(
    'http_requests_total',
    'Total HTTP requests',
    ['method', 'endpoint', 'status'],
    registry=registry
)

http_request_duration_seconds = Histogram(
    'http_request_duration_seconds',
    'HTTP request latency',
    ['method', 'endpoint'],
    registry=registry,
    buckets=(0.01, 0.05, 0.1, 0.5, 1.0, 2.5, 5.0, 10.0)
)

http_requests_in_progress = Gauge(
    'http_requests_in_progress',
    'HTTP requests in progress',
    ['method', 'endpoint'],
    registry=registry
)

# 数据库指标
db_connections_total = Gauge(
    'db_connections_total',
    'Total database connections',
    registry=registry
)

db_connections_active = Gauge(
    'db_connections_active',
    'Active database connections',
    registry=registry
)

db_query_duration_seconds = Histogram(
    'db_query_duration_seconds',
    'Database query duration',
    ['operation'],
    registry=registry,
    buckets=(0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0)
)

db_errors_total = Counter(
    'db_errors_total',
    'Total database errors',
    ['error_type'],
    registry=registry
)

# 缓存指标
cache_hits_total = Counter(
    'cache_hits_total',
    'Total cache hits',
    ['cache_type'],
    registry=registry
)

cache_misses_total = Counter(
    'cache_misses_total',
    'Total cache misses',
    ['cache_type'],
    registry=registry
)

cache_size_bytes = Gauge(
    'cache_size_bytes',
    'Current cache size in bytes',
    ['cache_type'],
    registry=registry
)

# 业务指标
active_users = Gauge(
    'active_users',
    'Number of active users',
    registry=registry
)

active_accounts = Gauge(
    'active_accounts',
    'Number of active trading accounts',
    registry=registry
)

total_positions = Gauge(
    'total_positions',
    'Total number of open positions',
    registry=registry
)

total_trades_today = Counter(
    'total_trades_today',
    'Total trades executed today',
    ['trade_type'],
    registry=registry
)

portfolio_value_usd = Gauge(
    'portfolio_value_usd',
    'Total portfolio value in USD',
    ['account_type'],
    registry=registry
)

# 系统指标
system_cpu_usage = Gauge(
    'system_cpu_usage_percent',
    'System CPU usage percentage',
    registry=registry
)

system_memory_usage = Gauge(
    'system_memory_usage_bytes',
    'System memory usage in bytes',
    registry=registry
)

system_memory_available = Gauge(
    'system_memory_available_bytes',
    'System available memory in bytes',
    registry=registry
)

system_disk_usage = Gauge(
    'system_disk_usage_bytes',
    'System disk usage in bytes',
    ['path'],
    registry=registry
)

# 应用指标
app_info = Info(
    'app',
    'Application information',
    registry=registry
)

app_uptime_seconds = Gauge(
    'app_uptime_seconds',
    'Application uptime in seconds',
    registry=registry
)

# 外部API指标
external_api_requests_total = Counter(
    'external_api_requests_total',
    'Total external API requests',
    ['api', 'endpoint', 'status'],
    registry=registry
)

external_api_duration_seconds = Histogram(
    'external_api_duration_seconds',
    'External API request duration',
    ['api', 'endpoint'],
    registry=registry,
    buckets=(0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0)
)

external_api_errors_total = Counter(
    'external_api_errors_total',
    'Total external API errors',
    ['api', 'error_type'],
    registry=registry
)

# Celery任务指标
celery_tasks_total = Counter(
    'celery_tasks_total',
    'Total Celery tasks',
    ['task_name', 'state'],
    registry=registry
)

celery_task_duration_seconds = Histogram(
    'celery_task_duration_seconds',
    'Celery task execution duration',
    ['task_name'],
    registry=registry
)

celery_queue_length = Gauge(
    'celery_queue_length',
    'Celery queue length',
    ['queue_name'],
    registry=registry
)


# ========== Prometheus中间件 ==========

class PrometheusMiddleware(BaseHTTPMiddleware):
    """Prometheus监控中间件"""
    
    async def dispatch(self, request: Request, call_next):
        # 跳过metrics端点自身
        if request.url.path == "/metrics":
            return await call_next(request)
        
        method = request.method
        endpoint = request.url.path
        
        # 请求开始
        http_requests_in_progress.labels(method=method, endpoint=endpoint).inc()
        start_time = time.time()
        
        try:
            response = await call_next(request)
            status = response.status_code
            
            # 记录指标
            duration = time.time() - start_time
            http_requests_total.labels(
                method=method,
                endpoint=endpoint,
                status=status
            ).inc()
            http_request_duration_seconds.labels(
                method=method,
                endpoint=endpoint
            ).observe(duration)
            
            return response
            
        except Exception as e:
            logger.error(f"Request error: {e}")
            http_requests_total.labels(
                method=method,
                endpoint=endpoint,
                status=500
            ).inc()
            raise
            
        finally:
            http_requests_in_progress.labels(method=method, endpoint=endpoint).dec()


# ========== 监控管理器 ==========

class MonitoringManager:
    """监控管理器"""
    
    def __init__(self):
        self.start_time = datetime.now()
        self._monitoring_task = None
        
        # 设置应用信息
        app_info.info({
            'version': getattr(settings, 'APP_VERSION', '1.0.0'),
            'environment': getattr(settings, 'ENVIRONMENT', 'development'),
            'name': 'fund_management_api'
        })
    
    async def start(self):
        """启动监控"""
        logger.info("Starting monitoring...")
        self._monitoring_task = asyncio.create_task(self._collect_system_metrics())
    
    async def stop(self):
        """停止监控"""
        if self._monitoring_task:
            self._monitoring_task.cancel()
            try:
                await self._monitoring_task
            except asyncio.CancelledError:
                pass
        logger.info("Monitoring stopped")
    
    async def _collect_system_metrics(self):
        """定期收集系统指标"""
        while True:
            try:
                # CPU使用率
                cpu_percent = psutil.cpu_percent(interval=1)
                system_cpu_usage.set(cpu_percent)
                
                # 内存使用
                memory = psutil.virtual_memory()
                system_memory_usage.set(memory.used)
                system_memory_available.set(memory.available)
                
                # 磁盘使用
                disk = psutil.disk_usage('/')
                system_disk_usage.labels(path='/').set(disk.used)
                
                # 应用运行时间
                uptime = (datetime.now() - self.start_time).total_seconds()
                app_uptime_seconds.set(uptime)
                
                # 每60秒收集一次
                await asyncio.sleep(60)
                
            except Exception as e:
                logger.error(f"Failed to collect system metrics: {e}")
                await asyncio.sleep(60)
    
    async def collect_business_metrics(self):
        """收集业务指标"""
        try:
            # TODO: 从数据库查询实际数据
            # from server.app.db import get_session
            # from server.app.services import get_statistics
            
            # 示例指标
            active_users.set(0)
            active_accounts.set(0)
            total_positions.set(0)
            
            logger.debug("Business metrics collected")
            
        except Exception as e:
            logger.error(f"Failed to collect business metrics: {e}")
    
    def record_db_query(self, operation: str, duration: float, error: Optional[str] = None):
        """记录数据库查询"""
        db_query_duration_seconds.labels(operation=operation).observe(duration)
        if error:
            db_errors_total.labels(error_type=error).inc()
    
    def record_cache_operation(self, cache_type: str, hit: bool):
        """记录缓存操作"""
        if hit:
            cache_hits_total.labels(cache_type=cache_type).inc()
        else:
            cache_misses_total.labels(cache_type=cache_type).inc()
    
    def record_external_api_call(
        self,
        api: str,
        endpoint: str,
        duration: float,
        status: int,
        error: Optional[str] = None
    ):
        """记录外部API调用"""
        external_api_requests_total.labels(
            api=api,
            endpoint=endpoint,
            status=status
        ).inc()
        external_api_duration_seconds.labels(
            api=api,
            endpoint=endpoint
        ).observe(duration)
        if error:
            external_api_errors_total.labels(
                api=api,
                error_type=error
            ).inc()
    
    def record_celery_task(self, task_name: str, state: str, duration: Optional[float] = None):
        """记录Celery任务"""
        celery_tasks_total.labels(task_name=task_name, state=state).inc()
        if duration:
            celery_task_duration_seconds.labels(task_name=task_name).observe(duration)


# 全局监控管理器实例
monitoring_manager = MonitoringManager()


# ========== 健康检查 ==========

class HealthCheck:
    """健康检查"""
    
    @staticmethod
    async def check_all() -> Dict[str, Any]:
        """执行所有健康检查"""
        checks = {
            "database": await HealthCheck.check_database(),
            "redis": await HealthCheck.check_redis(),
            "disk": await HealthCheck.check_disk(),
            "memory": await HealthCheck.check_memory(),
        }
        
        # 整体状态
        all_healthy = all(check["status"] == "healthy" for check in checks.values())
        
        return {
            "status": "healthy" if all_healthy else "unhealthy",
            "timestamp": datetime.now().isoformat(),
            "checks": checks
        }
    
    @staticmethod
    async def check_database() -> Dict[str, Any]:
        """检查数据库连接"""
        try:
            # TODO: 实现实际的数据库检查
            # from server.app.db import get_session
            # async with get_session() as session:
            #     await session.execute("SELECT 1")
            
            return {
                "status": "healthy",
                "latency_ms": 0,
            }
        except Exception as e:
            logger.error(f"Database health check failed: {e}")
            return {
                "status": "unhealthy",
                "error": str(e)
            }
    
    @staticmethod
    async def check_redis() -> Dict[str, Any]:
        """检查Redis连接"""
        try:
            # TODO: 实现Redis检查
            # from server.app.cache import cache_manager
            # await cache_manager.redis_client.ping()
            
            return {
                "status": "healthy",
                "latency_ms": 0,
            }
        except Exception as e:
            logger.error(f"Redis health check failed: {e}")
            return {
                "status": "unhealthy",
                "error": str(e)
            }
    
    @staticmethod
    async def check_disk() -> Dict[str, Any]:
        """检查磁盘空间"""
        try:
            disk = psutil.disk_usage('/')
            percent_used = disk.percent
            
            status = "healthy"
            if percent_used > 90:
                status = "critical"
            elif percent_used > 80:
                status = "warning"
            
            return {
                "status": status,
                "percent_used": percent_used,
                "free_gb": disk.free / (1024**3),
            }
        except Exception as e:
            logger.error(f"Disk health check failed: {e}")
            return {
                "status": "unknown",
                "error": str(e)
            }
    
    @staticmethod
    async def check_memory() -> Dict[str, Any]:
        """检查内存"""
        try:
            memory = psutil.virtual_memory()
            percent_used = memory.percent
            
            status = "healthy"
            if percent_used > 90:
                status = "critical"
            elif percent_used > 80:
                status = "warning"
            
            return {
                "status": status,
                "percent_used": percent_used,
                "available_gb": memory.available / (1024**3),
            }
        except Exception as e:
            logger.error(f"Memory health check failed: {e}")
            return {
                "status": "unknown",
                "error": str(e)
            }


# ========== 告警规则 ==========

class AlertRule:
    """告警规则"""
    
    def __init__(
        self,
        name: str,
        condition: callable,
        severity: str,
        message: str
    ):
        self.name = name
        self.condition = condition
        self.severity = severity
        self.message = message
    
    async def check(self) -> Optional[Dict[str, Any]]:
        """检查规则"""
        try:
            if await self.condition():
                return {
                    "rule": self.name,
                    "severity": self.severity,
                    "message": self.message,
                    "timestamp": datetime.now().isoformat()
                }
        except Exception as e:
            logger.error(f"Alert rule check failed: {e}")
        return None


class AlertManager:
    """告警管理器"""
    
    def __init__(self):
        self.rules = []
        self._setup_default_rules()
    
    def _setup_default_rules(self):
        """设置默认告警规则"""
        
        # CPU使用率告警
        self.rules.append(AlertRule(
            name="high_cpu_usage",
            condition=lambda: psutil.cpu_percent() > 80,
            severity="warning",
            message="CPU usage is above 80%"
        ))
        
        # 内存使用率告警
        self.rules.append(AlertRule(
            name="high_memory_usage",
            condition=lambda: psutil.virtual_memory().percent > 85,
            severity="warning",
            message="Memory usage is above 85%"
        ))
        
        # 磁盘空间告警
        self.rules.append(AlertRule(
            name="low_disk_space",
            condition=lambda: psutil.disk_usage('/').percent > 85,
            severity="critical",
            message="Disk space is above 85%"
        ))
    
    async def check_all_rules(self) -> list[Dict[str, Any]]:
        """检查所有规则"""
        alerts = []
        for rule in self.rules:
            alert = await rule.check()
            if alert:
                alerts.append(alert)
                logger.warning(f"Alert triggered: {alert}")
        return alerts
    
    def add_rule(self, rule: AlertRule):
        """添加告警规则"""
        self.rules.append(rule)


# 全局告警管理器
alert_manager = AlertManager()


# ========== 辅助函数 ==========

def get_metrics() -> bytes:
    """获取Prometheus指标"""
    return generate_latest(registry)


async def push_metrics_to_gateway(gateway_url: str, job_name: str):
    """推送指标到Pushgateway"""
    try:
        push_to_gateway(gateway_url, job=job_name, registry=registry)
        logger.debug(f"Metrics pushed to gateway: {gateway_url}")
    except Exception as e:
        logger.error(f"Failed to push metrics: {e}")
