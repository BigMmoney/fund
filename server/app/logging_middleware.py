"""
操作日志中间件和权限验证系统
基于需求文档的日志记录规范
"""
from datetime import datetime
from typing import Optional, Dict, Any
from fastapi import Request, Response, Depends
from sqlalchemy.orm import Session
import json
import asyncio

from server.app.database import get_db
from server.app.models import OperationLog, User
from server.app.auth import get_optional_current_user


class OperationLogger:
    """操作日志记录器"""
    
    @staticmethod
    async def log_operation(
        db: Session,
        user: Optional[User],
        operation: str,
        resource_type: Optional[str] = None,
        resource_id: Optional[str] = None,
        details: Optional[Dict[str, Any]] = None,
        request: Optional[Request] = None
    ):
        """记录操作日志"""
        try:
            # 获取IP地址和User Agent
            ip_address = None
            user_agent = None
            
            if request:
                ip_address = request.client.host if request.client else None
                user_agent = request.headers.get("user-agent")
            
            # 创建日志记录
            log_entry = OperationLog(
                user_id=user.id if user else None,
                operation=operation,
                resource_type=resource_type,
                resource_id=resource_id,
                details=details,
                ip_address=ip_address,
                user_agent=user_agent
            )
            
            db.add(log_entry)
            db.commit()
            
        except Exception as e:
            # 日志记录失败不应该影响主要操作
            print(f"Failed to log operation: {e}")
    
    @staticmethod
    def format_operation_details(
        operation_type: str,
        old_data: Optional[Dict] = None,
        new_data: Optional[Dict] = None,
        additional_info: Optional[Dict] = None
    ) -> Dict[str, Any]:
        """格式化操作详情"""
        details = {
            "operation_type": operation_type,
            "timestamp": datetime.utcnow().isoformat()
        }
        
        if old_data:
            details["old_data"] = old_data
        
        if new_data:
            details["new_data"] = new_data
        
        if additional_info:
            details.update(additional_info)
        
        return details


class LoggingMiddleware:
    """日志记录中间件"""
    
    def __init__(self):
        self.sensitive_paths = {
            "/auth/login", "/auth/password"
        }
        self.logged_operations = {
            "POST": "create",
            "PUT": "update", 
            "PATCH": "update",
            "DELETE": "delete"
        }
    
    async def __call__(self, request: Request, call_next, db: Session = Depends(get_db)):
        """中间件处理函数"""
        # 记录请求开始时间
        start_time = datetime.utcnow()
        
        # 获取用户信息（如果已认证）
        user = await get_optional_current_user(request, db)
        
        # 处理请求
        response = await call_next(request)
        
        # 计算处理时间
        process_time = (datetime.utcnow() - start_time).total_seconds()
        
        # 异步记录日志（不阻塞响应）
        asyncio.create_task(
            self._log_request(request, response, user, process_time, db)
        )
        
        return response
    
    async def _log_request(
        self, 
        request: Request, 
        response: Response, 
        user: Optional[User], 
        process_time: float,
        db: Session
    ):
        """记录请求日志"""
        try:
            method = request.method
            path = str(request.url.path)
            
            # 只记录需要权限的操作
            if method in self.logged_operations and user:
                operation = f"{method} {path}"
                
                # 提取资源信息
                resource_type, resource_id = self._extract_resource_info(path)
                
                # 创建详情信息
                details = {
                    "method": method,
                    "path": path,
                    "status_code": response.status_code,
                    "process_time": process_time,
                    "user_agent": request.headers.get("user-agent", ""),
                    "query_params": dict(request.query_params)
                }
                
                # 记录操作日志
                await OperationLogger.log_operation(
                    db=db,
                    user=user,
                    operation=operation,
                    resource_type=resource_type,
                    resource_id=resource_id,
                    details=details,
                    request=request
                )
        
        except Exception as e:
            print(f"Failed to log request: {e}")
    
    def _extract_resource_info(self, path: str) -> tuple[Optional[str], Optional[str]]:
        """从路径提取资源类型和ID"""
        parts = path.strip('/').split('/')
        
        if len(parts) >= 1:
            resource_type = parts[0]
            resource_id = None
            
            # 尝试找到资源ID（通常是数字）
            for part in parts[1:]:
                if part.isdigit():
                    resource_id = part
                    break
            
            return resource_type, resource_id
        
        return None, None


# 装饰器函数，用于手动记录特定操作
def log_operation(
    operation: str,
    resource_type: Optional[str] = None,
    get_resource_id: Optional[callable] = None,
    get_details: Optional[callable] = None
):
    """操作日志装饰器"""
    def decorator(func):
        async def wrapper(*args, **kwargs):
            # 执行原函数
            result = await func(*args, **kwargs)
            
            # 尝试从参数中获取数据库连接和用户
            db = kwargs.get('db')
            user = kwargs.get('user') or kwargs.get('current_user')
            request = kwargs.get('request')
            
            if db and user:
                # 获取资源ID
                resource_id = None
                if get_resource_id:
                    try:
                        resource_id = get_resource_id(*args, **kwargs, result=result)
                    except:
                        pass
                
                # 获取详情
                details = None
                if get_details:
                    try:
                        details = get_details(*args, **kwargs, result=result)
                    except:
                        pass
                
                # 异步记录日志
                asyncio.create_task(
                    OperationLogger.log_operation(
                        db=db,
                        user=user,
                        operation=operation,
                        resource_type=resource_type,
                        resource_id=str(resource_id) if resource_id else None,
                        details=details,
                        request=request
                    )
                )
            
            return result
        
        return wrapper
    return decorator


# 操作类型常量
class Operations:
    # 用户管理
    USER_CREATE = "user.create"
    USER_UPDATE = "user.update"
    USER_DELETE = "user.delete"
    USER_SUSPEND = "user.suspend"
    USER_RESET_PASSWORD = "user.reset_password"
    USER_UPDATE_PERMISSIONS = "user.update_permissions"
    
    # 团队管理
    TEAM_CREATE = "team.create"
    TEAM_UPDATE = "team.update"
    TEAM_DELETE = "team.delete"
    
    # 投资组合管理
    PORTFOLIO_CREATE = "portfolio.create"
    PORTFOLIO_UPDATE = "portfolio.update"
    PORTFOLIO_UPDATE_TEAM = "portfolio.update_team"
    PORTFOLIO_TEAM_BIND = "portfolio.team.bind"
    PORTFOLIO_TEAM_UNBIND = "portfolio.team.unbind"
    
    # 收益管理
    PROFIT_ALLOCATION_CREATE = "profit.allocation.create"
    PROFIT_WITHDRAWAL_CREATE = "profit.withdrawal.create"
    PROFIT_REALLOCATION_CREATE = "profit.reallocation.create"
    
    # 黑名单管理
    BLACKLIST_ADD = "blacklist.add"
    BLACKLIST_REMOVE = "blacklist.remove"
    
    # 认证相关
    AUTH_LOGIN = "auth.login"
    AUTH_LOGOUT = "auth.logout"
    AUTH_PASSWORD_CHANGE = "auth.password_change"


# 便捷函数：为常见操作创建日志记录器
async def log_user_operation(
    db: Session, 
    current_user: User, 
    operation: str, 
    target_user_id: Optional[int] = None,
    details: Optional[Dict] = None,
    request: Optional[Request] = None
):
    """记录用户相关操作"""
    await OperationLogger.log_operation(
        db=db,
        user=current_user,
        operation=operation,
        resource_type="user",
        resource_id=str(target_user_id) if target_user_id else None,
        details=details,
        request=request
    )


async def log_team_operation(
    db: Session, 
    current_user: User, 
    operation: str, 
    team_id: Optional[int] = None,
    details: Optional[Dict] = None,
    request: Optional[Request] = None
):
    """记录团队相关操作"""
    await OperationLogger.log_operation(
        db=db,
        user=current_user,
        operation=operation,
        resource_type="team",
        resource_id=str(team_id) if team_id else None,
        details=details,
        request=request
    )


async def log_portfolio_operation(
    db: Session, 
    current_user: User, 
    operation: str, 
    portfolio_id: Optional[int] = None,
    details: Optional[Dict] = None,
    request: Optional[Request] = None
):
    """记录投资组合相关操作"""
    await OperationLogger.log_operation(
        db=db,
        user=current_user,
        operation=operation,
        resource_type="portfolio",
        resource_id=str(portfolio_id) if portfolio_id else None,
        details=details,
        request=request
    )


async def log_profit_operation(
    db: Session, 
    current_user: User, 
    operation: str, 
    record_id: Optional[int] = None,
    details: Optional[Dict] = None,
    request: Optional[Request] = None
):
    """记录收益相关操作"""
    await OperationLogger.log_operation(
        db=db,
        user=current_user,
        operation=operation,
        resource_type="profit",
        resource_id=str(record_id) if record_id else None,
        details=details,
        request=request
    )