"""
P1功能测试套件
测试缓存、异步任务、监控等新功能
"""
import pytest
import asyncio
from unittest.mock import Mock, patch, AsyncMock
from datetime import datetime


# ========== 缓存测试 ==========

class TestCacheManager:
    """测试缓存管理器"""
    
    @pytest.mark.asyncio
    async def test_cache_connect(self):
        """测试缓存连接"""
        from server.app.cache import CacheManager
        
        cache = CacheManager()
        cache.enabled = False  # 禁用实际连接
        await cache.connect()
        
        assert cache.redis_client is None
    
    @pytest.mark.asyncio
    async def test_cache_set_get(self):
        """测试缓存读写"""
        from server.app.cache import CacheManager
        
        cache = CacheManager()
        cache.enabled = False
        
        # 模拟缓存操作
        result = await cache.get("test_key")
        assert result is None
        
        success = await cache.set("test_key", {"value": "test"})
        assert success is False  # 因为缓存被禁用
    
    @pytest.mark.asyncio
    async def test_cache_delete(self):
        """测试缓存删除"""
        from server.app.cache import CacheManager
        
        cache = CacheManager()
        cache.enabled = False
        
        result = await cache.delete("test_key")
        assert result is False
    
    @pytest.mark.asyncio
    async def test_cache_key_generation(self):
        """测试缓存键生成"""
        from server.app.cache import cache_key
        
        key1 = cache_key("user", 123, status="active")
        assert "user" in key1
        assert "123" in key1
        assert "status=active" in key1
        
        # 测试长键哈希
        long_key = cache_key("x" * 100, "y" * 100)
        assert len(long_key) < 250
        assert "hash" in long_key
    
    @pytest.mark.asyncio
    async def test_cached_decorator(self):
        """测试缓存装饰器"""
        from server.app.cache import cached
        
        call_count = 0
        
        @cached(ttl=300, prefix="test")
        async def test_function(value):
            nonlocal call_count
            call_count += 1
            return value * 2
        
        # 第一次调用
        result1 = await test_function(5)
        assert result1 == 10
        assert call_count == 1
    
    @pytest.mark.asyncio
    async def test_distributed_lock(self):
        """测试分布式锁"""
        from server.app.cache import DistributedLock, cache_manager
        
        cache_manager.enabled = False
        
        # 当缓存禁用时，锁应该失败
        lock = DistributedLock("test_lock", timeout=1, retry_times=1)
        
        with pytest.raises(Exception):
            async with lock:
                pass
    
    @pytest.mark.asyncio
    async def test_invalidate_cache(self):
        """测试批量清除缓存"""
        from server.app.cache import invalidate_cache
        
        patterns = ["user:*", "session:*"]
        deleted = await invalidate_cache(patterns)
        assert deleted >= 0


# ========== 异步任务测试 ==========

class TestCeleryTasks:
    """测试Celery任务"""
    
    def test_celery_app_config(self):
        """测试Celery应用配置"""
        from server.app.tasks import celery_app
        
        assert celery_app is not None
        assert celery_app.conf.task_serializer == 'json'
        assert celery_app.conf.result_serializer == 'json'
    
    def test_task_registration(self):
        """测试任务注册"""
        from server.app.tasks import (
            sync_all_positions,
            generate_daily_report,
            send_notification,
            check_alerts
        )
        
        assert sync_all_positions is not None
        assert generate_daily_report is not None
        assert send_notification is not None
        assert check_alerts is not None
    
    def test_sync_positions_task(self):
        """测试同步持仓任务"""
        from server.app.tasks import sync_all_positions
        
        # 执行任务（同步模式用于测试）
        result = sync_all_positions()
        
        assert result is not None
        assert result["status"] == "success"
        assert "timestamp" in result
    
    def test_generate_report_task(self):
        """测试生成报告任务"""
        from server.app.tasks import generate_daily_report
        
        result = generate_daily_report()
        
        assert result is not None
        assert result["report_type"] == "daily"
        assert result["status"] == "success"
    
    def test_send_notification_task(self):
        """测试发送通知任务"""
        from server.app.tasks import send_notification
        
        result = send_notification(
            user_id=1,
            notification_type="info",
            message="Test message",
            channels=["email"]
        )
        
        assert result is not None
        assert result["user_id"] == 1
        assert result["status"] == "sent"
    
    def test_check_alerts_task(self):
        """测试检查告警任务"""
        from server.app.tasks import check_alerts
        
        result = check_alerts()
        
        assert result is not None
        assert "checked_at" in result
        assert "alerts_count" in result
    
    def test_get_task_status(self):
        """测试获取任务状态"""
        from server.app.tasks import get_task_status
        
        status = get_task_status("test_task_id")
        
        assert status is not None
        assert "task_id" in status
        assert "state" in status
    
    def test_get_active_tasks(self):
        """测试获取活跃任务"""
        from server.app.tasks import get_active_tasks
        
        tasks = get_active_tasks()
        assert isinstance(tasks, list)
    
    def test_cleanup_task(self):
        """测试数据清理任务"""
        from server.app.tasks import cleanup_expired_data
        
        result = cleanup_expired_data()
        
        assert result is not None
        assert "logs_deleted" in result
        assert "cache_cleared" in result


# ========== 监控测试 ==========

class TestMonitoring:
    """测试监控功能"""
    
    def test_prometheus_metrics_defined(self):
        """测试Prometheus指标定义"""
        from server.app.monitoring import (
            http_requests_total,
            http_request_duration_seconds,
            db_connections_total,
            cache_hits_total,
            system_cpu_usage
        )
        
        assert http_requests_total is not None
        assert http_request_duration_seconds is not None
        assert db_connections_total is not None
        assert cache_hits_total is not None
        assert system_cpu_usage is not None
    
    @pytest.mark.asyncio
    async def test_monitoring_manager(self):
        """测试监控管理器"""
        from server.app.monitoring import MonitoringManager
        
        manager = MonitoringManager()
        assert manager.start_time is not None
        
        # 测试启动和停止
        await manager.start()
        await asyncio.sleep(0.1)
        await manager.stop()
    
    @pytest.mark.asyncio
    async def test_collect_business_metrics(self):
        """测试业务指标收集"""
        from server.app.monitoring import monitoring_manager
        
        await monitoring_manager.collect_business_metrics()
        # 应该正常完成，不抛出异常
    
    def test_record_db_query(self):
        """测试记录数据库查询"""
        from server.app.monitoring import monitoring_manager
        
        monitoring_manager.record_db_query("select", 0.05)
        monitoring_manager.record_db_query("insert", 0.1, error="timeout")
        # 应该正常完成
    
    def test_record_cache_operation(self):
        """测试记录缓存操作"""
        from server.app.monitoring import monitoring_manager
        
        monitoring_manager.record_cache_operation("redis", hit=True)
        monitoring_manager.record_cache_operation("redis", hit=False)
    
    def test_record_external_api_call(self):
        """测试记录外部API调用"""
        from server.app.monitoring import monitoring_manager
        
        monitoring_manager.record_external_api_call(
            api="onetoken",
            endpoint="/positions",
            duration=1.5,
            status=200
        )
        
        monitoring_manager.record_external_api_call(
            api="onetoken",
            endpoint="/positions",
            duration=2.0,
            status=500,
            error="timeout"
        )
    
    def test_record_celery_task(self):
        """测试记录Celery任务"""
        from server.app.monitoring import monitoring_manager
        
        monitoring_manager.record_celery_task("sync_positions", "SUCCESS", 5.2)
        monitoring_manager.record_celery_task("sync_positions", "FAILURE")
    
    @pytest.mark.asyncio
    async def test_health_check_all(self):
        """测试健康检查"""
        from server.app.monitoring import HealthCheck
        
        result = await HealthCheck.check_all()
        
        assert result is not None
        assert "status" in result
        assert "checks" in result
        assert "timestamp" in result
    
    @pytest.mark.asyncio
    async def test_health_check_database(self):
        """测试数据库健康检查"""
        from server.app.monitoring import HealthCheck
        
        result = await HealthCheck.check_database()
        
        assert result is not None
        assert "status" in result
    
    @pytest.mark.asyncio
    async def test_health_check_redis(self):
        """测试Redis健康检查"""
        from server.app.monitoring import HealthCheck
        
        result = await HealthCheck.check_redis()
        
        assert result is not None
        assert "status" in result
    
    @pytest.mark.asyncio
    async def test_health_check_disk(self):
        """测试磁盘健康检查"""
        from server.app.monitoring import HealthCheck
        
        result = await HealthCheck.check_disk()
        
        assert result is not None
        assert "status" in result
        assert "percent_used" in result
    
    @pytest.mark.asyncio
    async def test_health_check_memory(self):
        """测试内存健康检查"""
        from server.app.monitoring import HealthCheck
        
        result = await HealthCheck.check_memory()
        
        assert result is not None
        assert "status" in result
        assert "percent_used" in result
    
    @pytest.mark.asyncio
    async def test_alert_manager(self):
        """测试告警管理器"""
        from server.app.monitoring import AlertManager
        
        manager = AlertManager()
        assert len(manager.rules) > 0
        
        # 检查所有规则
        alerts = await manager.check_all_rules()
        assert isinstance(alerts, list)
    
    def test_get_metrics(self):
        """测试获取Prometheus指标"""
        from server.app.monitoring import get_metrics
        
        metrics = get_metrics()
        assert metrics is not None
        assert isinstance(metrics, bytes)


# ========== API版本控制测试 ==========

class TestAPIVersioning:
    """测试API版本控制"""
    
    def test_api_v1_router_exists(self):
        """测试API v1路由器"""
        from server.app.api_versioning import api_v1_router
        
        assert api_v1_router is not None
        assert api_v1_router.prefix == "/api/v1"
    
    def test_version_strategy(self):
        """测试版本检测策略"""
        from server.app.api_versioning import APIVersionStrategy
        
        # 测试从路径提取版本
        version = APIVersionStrategy.from_path("/api/v1/users")
        assert version == "v1"
        
        version = APIVersionStrategy.from_path("/api/v2/users")
        assert version == "v2"
        
        version = APIVersionStrategy.from_path("/users")
        assert version is None
    
    @pytest.mark.asyncio
    async def test_version_middleware(self):
        """测试版本中间件"""
        from server.app.api_versioning import APIVersionMiddleware
        from starlette.requests import Request
        from starlette.responses import Response
        
        async def dummy_call_next(request):
            return Response("OK")
        
        middleware = APIVersionMiddleware(None)
        
        # 创建模拟请求
        scope = {
            "type": "http",
            "method": "GET",
            "path": "/api/v1/test",
            "headers": [],
            "query_string": b"",
        }
        
        request = Request(scope)
        response = await middleware.dispatch(request, dummy_call_next)
        
        assert response is not None
        assert "X-API-Version" in response.headers


# ========== 集成测试 ==========

class TestIntegration:
    """集成测试"""
    
    @pytest.mark.asyncio
    async def test_cache_and_monitoring_integration(self):
        """测试缓存和监控集成"""
        from server.app.cache import cache_manager
        from server.app.monitoring import monitoring_manager
        
        # 模拟缓存操作并记录监控
        cache_manager.enabled = False
        await cache_manager.get("test_key")
        
        monitoring_manager.record_cache_operation("redis", hit=False)
        # 应该正常完成
    
    def test_tasks_and_monitoring_integration(self):
        """测试任务和监控集成"""
        from server.app.tasks import sync_all_positions
        from server.app.monitoring import monitoring_manager
        
        # 执行任务
        result = sync_all_positions()
        
        # 记录监控
        monitoring_manager.record_celery_task(
            "sync_all_positions",
            "SUCCESS",
            1.0
        )
        
        assert result["status"] == "success"
    
    @pytest.mark.asyncio
    async def test_full_stack_health_check(self):
        """测试全栈健康检查"""
        from server.app.monitoring import HealthCheck
        
        health = await HealthCheck.check_all()
        
        assert health is not None
        assert "status" in health
        assert "checks" in health
        
        # 应该至少检查了这些组件
        assert "database" in health["checks"]
        assert "redis" in health["checks"]
        assert "disk" in health["checks"]
        assert "memory" in health["checks"]


# ========== 性能测试 ==========

class TestPerformance:
    """性能测试"""
    
    @pytest.mark.asyncio
    async def test_cache_performance(self):
        """测试缓存性能"""
        import time
        from server.app.cache import cache_key
        
        start = time.time()
        
        # 生成1000个缓存键
        for i in range(1000):
            key = cache_key("test", i, status="active")
            assert key is not None
        
        duration = time.time() - start
        assert duration < 1.0  # 应该在1秒内完成
    
    def test_metrics_performance(self):
        """测试指标收集性能"""
        import time
        from server.app.monitoring import monitoring_manager
        
        start = time.time()
        
        # 记录1000次指标
        for i in range(1000):
            monitoring_manager.record_db_query("select", 0.001)
            monitoring_manager.record_cache_operation("redis", hit=True)
        
        duration = time.time() - start
        assert duration < 2.0  # 应该在2秒内完成


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
