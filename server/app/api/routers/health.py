from fastapi import APIRouter
from datetime import datetime
import sys

router = APIRouter()

@router.get("/health")
async def health_check():
    """基础健康检查"""
    return {"status": "ok", "time": datetime.utcnow().isoformat()}


@router.get("/health/memory")
async def memory_health_check():
    """
    内存使用情况健康检查
    返回详细的内存统计信息和OOM风险评估
    """
    try:
        # 导入内存监控器
        from app.core.memory_monitor import memory_monitor, get_memory_status, cleanup_memory
        
        # 获取当前内存信息
        memory_info = get_memory_status()
        
        # 获取垃圾回收统计
        gc_stats = memory_monitor.get_gc_stats()
        
        # 评估OOM风险
        memory_percent = memory_info.get('process', {}).get('percent', 0)
        rss_mb = memory_info.get('process', {}).get('rss_mb', 0)
        
        risk_level = "low"
        recommendations = []
        
        if memory_percent >= 85:
            risk_level = "critical"
            recommendations.append("立即执行内存清理")
            recommendations.append("考虑重启服务")
            recommendations.append("检查是否有内存泄漏")
        elif memory_percent >= 70:
            risk_level = "high"
            recommendations.append("建议执行垃圾回收")
            recommendations.append("监控内存增长趋势")
        elif memory_percent >= 50:
            risk_level = "medium"
            recommendations.append("定期监控内存使用")
        else:
            recommendations.append("内存使用正常")
        
        return {
            "status": memory_info.get('status', 'unknown'),
            "memory": memory_info,
            "gc_stats": gc_stats,
            "risk_assessment": {
                "level": risk_level,
                "recommendations": recommendations
            },
            "thresholds": {
                "warning": f"{memory_monitor.warning_threshold}%",
                "critical": f"{memory_monitor.critical_threshold}%"
            }
        }
        
    except ImportError:
        # 如果内存监控器不可用，使用基础的内存检查
        import psutil
        import os
        
        process = psutil.Process(os.getpid())
        mem_info = process.memory_info()
        
        return {
            "status": "ok",
            "memory": {
                "rss_mb": round(mem_info.rss / 1024 / 1024, 2),
                "vms_mb": round(mem_info.vms / 1024 / 1024, 2),
                "percent": round(process.memory_percent(), 2)
            },
            "note": "Advanced memory monitoring not available"
        }
    except Exception as e:
        return {
            "status": "error",
            "error": str(e)
        }


@router.post("/health/memory/cleanup")
async def force_memory_cleanup():
    """
    强制执行内存清理
    触发垃圾回收，释放未使用的内存
    """
    try:
        from app.core.memory_monitor import cleanup_memory
        
        result = cleanup_memory()
        
        return {
            "status": "success",
            "message": "Memory cleanup completed",
            "result": result
        }
        
    except Exception as e:
        return {
            "status": "error",
            "error": str(e)
        }


@router.get("/health/database")
async def database_health_check():
    """
    数据库连接池健康检查
    返回连接池状态和性能指标
    """
    try:
        # 尝试使用优化的数据库管理器
        from app.core.database_optimized import db_manager
        
        # 获取连接池状态
        pool_status = db_manager.get_pool_status()
        
        # 执行健康检查
        health_status = db_manager.health_check()
        
        return {
            "status": "healthy" if health_status.get('overall') else "unhealthy",
            "pool_status": pool_status,
            "health_check": health_status,
            "timestamp": datetime.utcnow().isoformat()
        }
        
    except ImportError:
        # 使用旧的数据库连接
        from app.database import check_database_connection
        
        is_healthy = check_database_connection()
        
        return {
            "status": "healthy" if is_healthy else "unhealthy",
            "connection": is_healthy,
            "note": "Using legacy database connection"
        }
    except Exception as e:
        return {
            "status": "error",
            "error": str(e)
        }


@router.get("/health/onetoken")
async def onetoken_health_check():
    """
    OneToken API 健康检查
    测试 OneToken API 连接状态和响应时间
    """
    try:
        from server.app.services.onetoken_client import OneTokenClient
        
        client = OneTokenClient()
        start_time = datetime.utcnow()
        
        # 调用简单的 API 方法测试连接
        result = client.get_portfolios()
        
        elapsed = (datetime.utcnow() - start_time).total_seconds()
        
        # OneToken API 成功时返回 message="success" 和 result
        if result and result.get('message') == 'success' and result.get('result'):
            portfolio_count = len(result.get('result', {}).get('fund_info_list', []))
            return {
                "status": "healthy",
                "message": f"OneToken API is responding ({portfolio_count} portfolios)",
                "response_time_ms": int(elapsed * 1000),
                "last_check": datetime.utcnow().isoformat() + "Z"
            }
        else:
            return {
                "status": "unhealthy",
                "message": "OneToken API returned unexpected response",
                "response_time_ms": int(elapsed * 1000),
                "details": result
            }
    except Exception as e:
        return {
            "status": "error",
            "message": f"OneToken health check failed: {str(e)}"
        }


@router.get("/health/ceffu")
async def ceffu_health_check():
    """
    Ceffu API 健康检查
    测试 Ceffu API 连接状态和响应时间
    """
    try:
        from server.app.services.ceffu_client import CeffuClient
        
        client = CeffuClient()
        start_time = datetime.utcnow()
        
        # 调用系统状态端点
        result = client.get_system_status()
        
        elapsed = (datetime.utcnow() - start_time).total_seconds()
        
        if result and result.get('code') == '000000':
            data = result.get('data', {})
            return {
                "status": "healthy",
                "message": "Ceffu API is responding",
                "response_time_ms": int(elapsed * 1000),
                "system_status": data.get('message', 'Normal'),
                "last_check": datetime.utcnow().isoformat() + "Z"
            }
        else:
            return {
                "status": "unhealthy",
                "message": "Ceffu API returned unexpected response",
                "response_time_ms": int(elapsed * 1000),
                "details": result
            }
    except Exception as e:
        return {
            "status": "error",
            "message": f"Ceffu health check failed: {str(e)}"
        }


@router.get("/health/full")
async def full_health_check():
    """
    完整的系统健康检查
    包含应用、内存、数据库、OneToken、Ceffu的全面状态
    """
    import platform
    
    # 基础健康状态
    health = {
        "status": "healthy",
        "timestamp": datetime.utcnow().isoformat(),
        "uptime_seconds": None
    }
    
    # 系统信息
    try:
        import psutil
        import os
        
        process = psutil.Process(os.getpid())
        boot_time = datetime.fromtimestamp(psutil.boot_time())
        
        health["system"] = {
            "platform": platform.system(),
            "python_version": sys.version.split()[0],
            "cpu_count": psutil.cpu_count(),
            "cpu_percent": psutil.cpu_percent(interval=1),
            "boot_time": boot_time.isoformat()
        }
        
        # 计算进程运行时间
        create_time = datetime.fromtimestamp(process.create_time())
        uptime = datetime.now() - create_time
        health["uptime_seconds"] = int(uptime.total_seconds())
        
    except Exception as e:
        health["system_error"] = str(e)
    
    # 内存状态
    try:
        memory_check = await memory_health_check()
        health["memory"] = memory_check
    except Exception as e:
        health["memory"] = {"status": "error", "error": str(e)}
    
    # 数据库状态
    try:
        db_check = await database_health_check()
        health["database"] = db_check
    except Exception as e:
        health["database"] = {"status": "error", "error": str(e)}
    
    # OneToken API 状态
    try:
        onetoken_check = await onetoken_health_check()
        health["onetoken"] = onetoken_check
    except Exception as e:
        health["onetoken"] = {"status": "error", "error": str(e)}
    
    # Ceffu API 状态
    try:
        ceffu_check = await ceffu_health_check()
        health["ceffu"] = ceffu_check
    except Exception as e:
        health["ceffu"] = {"status": "error", "error": str(e)}
    
    # 评估整体健康状态
    if health.get("memory", {}).get("status") in ["critical", "error"]:
        health["status"] = "critical"
    elif health.get("database", {}).get("status") in ["unhealthy", "error"]:
        health["status"] = "degraded"
    elif health.get("onetoken", {}).get("status") in ["unhealthy", "error"]:
        health["status"] = "degraded"
    elif health.get("ceffu", {}).get("status") in ["unhealthy", "error"]:
        health["status"] = "degraded"
    elif health.get("memory", {}).get("status") == "warning":
        health["status"] = "warning"
    
    return health