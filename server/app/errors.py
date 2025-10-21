"""
统一错误码和异常处理体系
"""
from enum import Enum
from typing import Optional, Dict, Any
import uuid


class ErrorCode(Enum):
    """
    统一错误码定义
    
    错误码格式: (code, message, http_status)
    - 1xxx: 认证和授权错误
    - 2xxx: 业务逻辑错误
    - 3xxx: 外部服务错误
    - 4xxx: 数据库错误
    - 5xxx: 系统错误
    """
    
    # ========== 1xxx 认证和授权错误 ==========
    AUTH_INVALID_CREDENTIALS = (1001, "用户名或密码错误", 401)
    AUTH_TOKEN_EXPIRED = (1002, "登录已过期，请重新登录", 401)
    AUTH_TOKEN_INVALID = (1003, "无效的认证令牌", 401)
    AUTH_INSUFFICIENT_PERMISSIONS = (1004, "权限不足，无法执行此操作", 403)
    AUTH_ACCOUNT_LOCKED = (1005, "账户已被锁定", 403)
    AUTH_ACCOUNT_INACTIVE = (1006, "账户未激活", 403)
    AUTH_PASSWORD_TOO_WEAK = (1007, "密码强度不足", 400)
    AUTH_EMAIL_NOT_VERIFIED = (1008, "邮箱未验证", 403)
    
    # ========== 2xxx 业务逻辑错误 ==========
    # 用户相关
    USER_NOT_FOUND = (2001, "用户不存在", 404)
    USER_ALREADY_EXISTS = (2002, "用户已存在", 409)
    USER_EMAIL_DUPLICATE = (2003, "邮箱已被注册", 409)
    USER_PHONE_DUPLICATE = (2004, "手机号已被注册", 409)
    
    # 投资组合相关
    PORTFOLIO_NOT_FOUND = (2101, "投资组合不存在", 404)
    PORTFOLIO_CODE_DUPLICATE = (2102, "投资组合代码已存在", 409)
    PORTFOLIO_CANNOT_DELETE = (2103, "投资组合无法删除，请先清空资产", 400)
    PORTFOLIO_INSUFFICIENT_BALANCE = (2104, "投资组合余额不足", 400)
    PORTFOLIO_ALREADY_CLOSED = (2105, "投资组合已关闭", 400)
    
    # 收益分配相关
    ALLOCATION_RATIO_INVALID = (2201, "分配比例不正确，总和必须为10000", 400)
    ALLOCATION_RATIO_NEGATIVE = (2202, "分配比例不能为负数", 400)
    ALLOCATION_VERSION_CONFLICT = (2203, "分配比例版本冲突，请刷新后重试", 409)
    ALLOCATION_NOT_FOUND = (2204, "分配比例配置不存在", 404)
    
    # 提现相关
    WITHDRAWAL_AMOUNT_INVALID = (2301, "提现金额无效", 400)
    WITHDRAWAL_AMOUNT_EXCEEDS_AVAILABLE = (2302, "提现金额超过可用余额", 400)
    WITHDRAWAL_MINIMUM_NOT_MET = (2303, "提现金额低于最小限额", 400)
    WITHDRAWAL_DAILY_LIMIT_EXCEEDED = (2304, "已超过每日提现限额", 400)
    WITHDRAWAL_PENDING = (2305, "有提现申请正在处理中", 400)
    WITHDRAWAL_FAILED = (2306, "提现失败", 500)
    
    # 团队相关
    TEAM_NOT_FOUND = (2401, "交易团队不存在", 404)
    TEAM_CODE_DUPLICATE = (2402, "交易团队代码已存在", 409)
    TEAM_MEMBER_LIMIT_EXCEEDED = (2403, "团队成员数量已达上限", 400)
    
    # 数据验证
    VALIDATION_ERROR = (2501, "数据验证失败", 422)
    INVALID_DATE_RANGE = (2502, "日期范围无效", 400)
    INVALID_PARAMETER = (2503, "参数无效", 400)
    MISSING_REQUIRED_FIELD = (2504, "缺少必填字段", 400)
    
    # ========== 3xxx 外部服务错误 ==========
    ONETOKEN_API_ERROR = (3001, "OneToken服务暂时不可用，请稍后重试", 503)
    ONETOKEN_AUTH_FAILED = (3002, "OneToken认证失败", 500)
    ONETOKEN_TIMEOUT = (3003, "OneToken服务响应超时", 504)
    ONETOKEN_RATE_LIMIT = (3004, "OneToken API调用频率超限", 429)
    
    CEFFU_API_ERROR = (3101, "Ceffu服务暂时不可用，请稍后重试", 503)
    CEFFU_AUTH_FAILED = (3102, "Ceffu认证失败", 500)
    CEFFU_TIMEOUT = (3103, "Ceffu服务响应超时", 504)
    CEFFU_INSUFFICIENT_BALANCE = (3104, "Ceffu钱包余额不足", 400)
    
    EXTERNAL_SERVICE_ERROR = (3900, "外部服务错误", 503)
    EXTERNAL_SERVICE_TIMEOUT = (3901, "外部服务响应超时", 504)
    EXTERNAL_SERVICE_UNAVAILABLE = (3902, "外部服务不可用", 503)
    
    # ========== 4xxx 数据库错误 ==========
    DATABASE_CONNECTION_ERROR = (4001, "数据库连接失败", 500)
    DATABASE_QUERY_ERROR = (4002, "数据查询失败", 500)
    DATABASE_CONSTRAINT_VIOLATION = (4003, "数据约束冲突", 409)
    DATABASE_DEADLOCK = (4004, "数据库死锁，请重试", 500)
    DATABASE_TIMEOUT = (4005, "数据库操作超时", 504)
    DATABASE_INTEGRITY_ERROR = (4006, "数据完整性错误", 409)
    
    # ========== 5xxx 系统错误 ==========
    INTERNAL_SERVER_ERROR = (5001, "系统内部错误", 500)
    CONFIGURATION_ERROR = (5002, "系统配置错误", 500)
    SERVICE_UNAVAILABLE = (5003, "服务暂时不可用", 503)
    NOT_IMPLEMENTED = (5004, "功能暂未实现", 501)
    MAINTENANCE_MODE = (5005, "系统维护中", 503)
    
    # 文件相关
    FILE_NOT_FOUND = (5101, "文件不存在", 404)
    FILE_TOO_LARGE = (5102, "文件过大", 413)
    FILE_TYPE_NOT_ALLOWED = (5103, "文件类型不允许", 400)
    
    # 限流相关
    RATE_LIMIT_EXCEEDED = (5201, "请求过于频繁，请稍后重试", 429)
    
    @property
    def code(self) -> int:
        """错误码"""
        return self.value[0]
    
    @property
    def message(self) -> str:
        """错误消息"""
        return self.value[1]
    
    @property
    def http_status(self) -> int:
        """HTTP状态码"""
        return self.value[2]


class ApplicationError(Exception):
    """
    应用基础异常类
    
    所有业务异常都应继承此类
    """
    
    def __init__(
        self,
        error_code: ErrorCode,
        details: Optional[str] = None,
        user_message: Optional[str] = None,
        context: Optional[Dict[str, Any]] = None,
        **kwargs
    ):
        """
        Args:
            error_code: 错误码枚举
            details: 技术详情（用于日志，不展示给用户）
            user_message: 用户友好的错误消息（可选，默认使用error_code的message）
            context: 错误上下文信息（用于日志和调试）
            **kwargs: 额外的上下文信息
        """
        self.error_code = error_code
        self.details = details
        self.user_message = user_message or error_code.message
        self.context = context or {}
        self.context.update(kwargs)
        self.trace_id = str(uuid.uuid4())
        
        super().__init__(self.user_message)
    
    def to_dict(self) -> dict:
        """转换为字典格式"""
        return {
            "errorCode": self.error_code.code,
            "message": self.user_message,
            "traceId": self.trace_id,
            "details": self.details if self.details else None,
        }
    
    def __str__(self) -> str:
        return (
            f"{self.__class__.__name__}("
            f"code={self.error_code.code}, "
            f"message={self.user_message}, "
            f"trace_id={self.trace_id})"
        )
    
    def __repr__(self) -> str:
        return self.__str__()


# ========== 具体异常类 ==========

class AuthenticationError(ApplicationError):
    """认证错误"""
    pass


class AuthorizationError(ApplicationError):
    """授权错误"""
    pass


class BusinessLogicError(ApplicationError):
    """业务逻辑错误"""
    pass


class ValidationError(ApplicationError):
    """数据验证错误"""
    def __init__(self, message: str, field: Optional[str] = None, **kwargs):
        super().__init__(
            error_code=ErrorCode.VALIDATION_ERROR,
            user_message=message,
            field=field,
            **kwargs
        )


class NotFoundError(ApplicationError):
    """资源不存在错误"""
    def __init__(self, resource: str, identifier: Any, **kwargs):
        super().__init__(
            error_code=ErrorCode.USER_NOT_FOUND,  # 默认，可被覆盖
            user_message=f"{resource}不存在",
            resource=resource,
            identifier=identifier,
            **kwargs
        )


class ConflictError(ApplicationError):
    """资源冲突错误"""
    pass


class ExternalServiceError(ApplicationError):
    """外部服务错误"""
    pass


class DatabaseError(ApplicationError):
    """数据库错误"""
    pass


class RateLimitError(ApplicationError):
    """限流错误"""
    def __init__(self, retry_after: Optional[int] = None, **kwargs):
        super().__init__(
            error_code=ErrorCode.RATE_LIMIT_EXCEEDED,
            retry_after=retry_after,
            **kwargs
        )


# ========== 便捷工具函数 ==========

def raise_not_found(resource: str, identifier: Any, **kwargs):
    """
    抛出资源不存在异常
    
    用法:
        raise_not_found("Portfolio", portfolio_id)
    """
    raise NotFoundError(resource, identifier, **kwargs)


def raise_validation_error(message: str, field: Optional[str] = None, **kwargs):
    """
    抛出验证错误
    
    用法:
        raise_validation_error("邮箱格式不正确", field="email")
    """
    raise ValidationError(message, field=field, **kwargs)


def raise_auth_error(error_code: ErrorCode = ErrorCode.AUTH_INVALID_CREDENTIALS, **kwargs):
    """
    抛出认证错误
    
    用法:
        raise_auth_error(ErrorCode.AUTH_TOKEN_EXPIRED)
    """
    raise AuthenticationError(error_code, **kwargs)


def raise_business_error(error_code: ErrorCode, details: Optional[str] = None, **kwargs):
    """
    抛出业务逻辑错误
    
    用法:
        raise_business_error(
            ErrorCode.ALLOCATION_RATIO_INVALID,
            details="Sum is 9500, expected 10000"
        )
    """
    raise BusinessLogicError(error_code, details=details, **kwargs)


def raise_external_service_error(
    service_name: str,
    error_code: ErrorCode = ErrorCode.EXTERNAL_SERVICE_ERROR,
    **kwargs
):
    """
    抛出外部服务错误
    
    用法:
        raise_external_service_error("OneToken", ErrorCode.ONETOKEN_TIMEOUT)
    """
    raise ExternalServiceError(error_code, service=service_name, **kwargs)


# ========== 错误响应构建器 ==========

class ErrorResponseBuilder:
    """错误响应构建器"""
    
    @staticmethod
    def build_error_response(
        error: ApplicationError,
        include_details: bool = False
    ) -> dict:
        """
        构建错误响应
        
        Args:
            error: 应用异常
            include_details: 是否包含技术详情（生产环境应为False）
        """
        response = {
            "isOK": False,
            "message": error.user_message,
            "data": {
                "errorCode": error.error_code.code,
                "traceId": error.trace_id,
            }
        }
        
        if include_details and error.details:
            response["data"]["details"] = error.details
        
        return response
    
    @staticmethod
    def build_validation_error_response(
        validation_errors: list
    ) -> dict:
        """
        构建验证错误响应
        
        Args:
            validation_errors: Pydantic验证错误列表
        """
        return {
            "isOK": False,
            "message": "数据验证失败",
            "data": {
                "errorCode": ErrorCode.VALIDATION_ERROR.code,
                "errors": [
                    {
                        "field": ".".join(str(loc) for loc in error["loc"]),
                        "message": error["msg"],
                        "type": error["type"]
                    }
                    for error in validation_errors
                ]
            }
        }
