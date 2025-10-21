"""
API限流配置
使用slowapi实现基于Redis的限流
"""
from slowapi import Limiter, _rate_limit_exceeded_handler
from slowapi.util import get_remote_address
from slowapi.errors import RateLimitExceeded
from fastapi import Request
import redis
from typing import Optional
from loguru import logger


# ========== 限流键函数 ==========

def get_remote_address_or_forward(request: Request) -> str:
    """
    获取客户端IP地址
    优先从X-Forwarded-For获取（代理/负载均衡场景）
    """
    forwarded = request.headers.get("X-Forwarded-For")
    if forwarded:
        return forwarded.split(",")[0].strip()
    return get_remote_address(request)


def get_user_id_from_token(request: Request) -> str:
    """
    从JWT token中提取用户ID用于限流
    未认证用户使用IP地址
    """
    try:
        # 尝试从请求状态中获取用户信息
        if hasattr(request.state, "user") and request.state.user:
            return f"user_{request.state.user.id}"
    except:
        pass
    
    # 尝试从Authorization头解析
    auth_header = request.headers.get("Authorization", "")
    if auth_header.startswith("Bearer "):
        token = auth_header[7:]
        try:
            # 使用JWT工具解码token获取用户ID
            from server.app.jwt_utils import decode_access_token
            payload = decode_access_token(token)
            if payload and "sub" in payload:
                return f"user_{payload['sub']}"
        except Exception as e:
            # Token解码失败，继续使用IP限流
            pass
    
    # 回退到IP地址
    return get_remote_address_or_forward(request)


# ========== 限流器配置 ==========

def create_limiter(redis_url: Optional[str] = None) -> Limiter:
    """
    创建限流器实例
    
    Args:
        redis_url: Redis连接URL（如果为None，使用内存存储）
    """
    if redis_url:
        try:
            # 测试Redis连接
            r = redis.from_url(redis_url)
            r.ping()
            logger.info(f"Rate limiter using Redis: {redis_url}")
            
            limiter = Limiter(
                key_func=get_remote_address_or_forward,
                storage_uri=redis_url,
                default_limits=["1000 per hour", "100 per minute"],
                headers_enabled=True,  # 在响应头中返回限流信息
                config_filename=None,  # 禁用.env加载
            )
        except Exception as e:
            logger.warning(f"Failed to connect to Redis for rate limiting: {e}")
            logger.warning("Falling back to in-memory rate limiting (not recommended for production)")
            limiter = Limiter(
                key_func=get_remote_address_or_forward,
                default_limits=["1000 per hour", "100 per minute"],
                config_filename=None,  # 禁用.env加载
            )
    else:
        logger.warning("Using in-memory rate limiting (not suitable for production)")
        # 禁用.env文件加载以避免编码问题
        limiter = Limiter(
            key_func=get_remote_address_or_forward,
            default_limits=["1000 per hour", "100 per minute"],
            config_filename=None,  # 禁用.env加载
        )
    
    return limiter


# ========== 预定义限流规则 ==========

class RateLimitRules:
    """预定义的限流规则"""
    
    # 严格限流（登录、注册等敏感操作）
    STRICT = "5 per minute"
    
    # 中等限流（数据修改操作）
    MODERATE = "20 per minute"
    
    # 宽松限流（查询操作）
    LENIENT = "100 per minute"
    
    # 每日限流（提现等操作）
    DAILY_LIMIT = "10 per day"
    
    # 外部API调用全局限流
    EXTERNAL_API_GLOBAL = "1000 per hour"


# ========== 基于用户的限流器 ==========

def create_user_limiter(redis_url: Optional[str] = None) -> Limiter:
    """
    创建基于用户的限流器
    
    用于需要区分用户的限流场景
    """
    if redis_url:
        return Limiter(
            key_func=get_user_id_from_token,
            storage_uri=redis_url,
            default_limits=["1000 per hour"],
            headers_enabled=True,
        )
    else:
        return Limiter(
            key_func=get_user_id_from_token,
            default_limits=["1000 per hour"],
        )


# ========== 自定义限流异常处理 ==========

async def custom_rate_limit_handler(request: Request, exc: RateLimitExceeded):
    """
    自定义限流异常响应
    
    返回StandardResponse格式
    """
    from fastapi.responses import JSONResponse
    
    # 获取重试时间
    retry_after = exc.detail.split("Retry after ")[1] if "Retry after" in exc.detail else "60 seconds"
    
    logger.warning(
        f"Rate limit exceeded for {request.url.path}",
        extra={
            "path": request.url.path,
            "method": request.method,
            "client_ip": get_remote_address_or_forward(request),
            "retry_after": retry_after,
        }
    )
    
    return JSONResponse(
        status_code=429,
        content={
            "isOK": False,
            "message": "请求过于频繁，请稍后重试",
            "data": {
                "errorCode": 5201,
                "retryAfter": retry_after,
            }
        },
        headers={
            "Retry-After": retry_after,
        }
    )


# ========== 装饰器示例 ==========

"""
使用示例:

from app.rate_limit import limiter, RateLimitRules
from fastapi import Request

@router.post("/auth/login")
@limiter.limit(RateLimitRules.STRICT)  # 每分钟最多5次
async def login(request: Request, login_data: dict):
    pass

@router.get("/portfolios")
@limiter.limit(RateLimitRules.LENIENT)  # 每分钟最多100次
async def get_portfolios(request: Request):
    pass

@router.post("/withdrawals")
@limiter.limit(RateLimitRules.DAILY_LIMIT)  # 每天最多10次
async def create_withdrawal(request: Request):
    pass
"""


def setup_rate_limiting(app, redis_url: Optional[str] = None):
    """
    设置限流
    
    用法:
        from app.main import app
        from app.rate_limit import setup_rate_limiting
        setup_rate_limiting(app, redis_url="redis://localhost:6379/0")
    """
    limiter = create_limiter(redis_url)
    
    # 将limiter绑定到app.state
    app.state.limiter = limiter
    
    # 注册异常处理器
    app.add_exception_handler(RateLimitExceeded, custom_rate_limit_handler)
    
    logger.info("Rate limiting configured successfully")
    
    return limiter
