"""
安全和认证核心模块 - JWT、密码加密、权限验证
"""
import hashlib
import hmac
import secrets
import jwt
import bcrypt
from datetime import datetime, timedelta
from typing import Optional, Dict, Any, List
from passlib.context import CryptContext
from fastapi import HTTPException, Depends, status
from fastapi.security import HTTPBearer, HTTPAuthorizationCredentials
from sqlalchemy.orm import Session

from server.app.config import settings
from server.app.core.database import get_database_session

import logging
logger = logging.getLogger(__name__)

# 密码加密上下文
pwd_context = CryptContext(schemes=["bcrypt"], deprecated="auto")

# HTTP Bearer token 方案
security = HTTPBearer(auto_error=False)

class SecurityManager:
    """安全管理器 - 处理加密、JWT、权限验证"""
    
    def __init__(self):
        self.secret_key = settings.security.SECRET_KEY
        self.algorithm = settings.security.ALGORITHM
        self.access_token_expire_minutes = settings.security.ACCESS_TOKEN_EXPIRE_MINUTES
        self.refresh_token_expire_days = settings.security.REFRESH_TOKEN_EXPIRE_DAYS
    
    # ==================== 密码相关 ====================
    
    def hash_password(self, password: str) -> str:
        """密码哈希"""
        try:
            return pwd_context.hash(password)
        except Exception as e:
            logger.error(f"Password hashing failed: {e}")
            raise ValueError("Password hashing failed")
    
    def verify_password(self, plain_password: str, hashed_password: str) -> bool:
        """验证密码"""
        try:
            return pwd_context.verify(plain_password, hashed_password)
        except Exception as e:
            logger.error(f"Password verification failed: {e}")
            return False
    
    def validate_password_strength(self, password: str) -> List[str]:
        """验证密码强度，返回错误列表"""
        errors = []
        
        if len(password) < settings.security.PASSWORD_MIN_LENGTH:
            errors.append(f"Password must be at least {settings.security.PASSWORD_MIN_LENGTH} characters long")
        
        if settings.security.PASSWORD_REQUIRE_UPPERCASE and not any(c.isupper() for c in password):
            errors.append("Password must contain at least one uppercase letter")
        
        if settings.security.PASSWORD_REQUIRE_LOWERCASE and not any(c.islower() for c in password):
            errors.append("Password must contain at least one lowercase letter")
        
        if settings.security.PASSWORD_REQUIRE_DIGITS and not any(c.isdigit() for c in password):
            errors.append("Password must contain at least one digit")
        
        if settings.security.PASSWORD_REQUIRE_SPECIAL and not any(c in "!@#$%^&*()_+-=[]{}|;:,.<>?" for c in password):
            errors.append("Password must contain at least one special character")
        
        return errors
    
    def generate_secure_password(self, length: int = 12) -> str:
        """生成安全密码"""
        alphabet = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!@#$%^&*"
        password = ''.join(secrets.choice(alphabet) for _ in range(length))
        
        # 确保包含所有必需的字符类型
        if settings.security.PASSWORD_REQUIRE_UPPERCASE and not any(c.isupper() for c in password):
            password = password[:-1] + secrets.choice("ABCDEFGHIJKLMNOPQRSTUVWXYZ")
        
        if settings.security.PASSWORD_REQUIRE_LOWERCASE and not any(c.islower() for c in password):
            password = password[:-1] + secrets.choice("abcdefghijklmnopqrstuvwxyz")
        
        if settings.security.PASSWORD_REQUIRE_DIGITS and not any(c.isdigit() for c in password):
            password = password[:-1] + secrets.choice("0123456789")
        
        return password
    
    # ==================== JWT Token相关 ====================
    
    def create_access_token(self, data: Dict[str, Any], expires_delta: Optional[timedelta] = None) -> str:
        """创建访问令牌"""
        to_encode = data.copy()
        
        if expires_delta:
            expire = datetime.utcnow() + expires_delta
        else:
            expire = datetime.utcnow() + timedelta(minutes=self.access_token_expire_minutes)
        
        to_encode.update({
            "exp": expire,
            "iat": datetime.utcnow(),
            "type": "access"
        })
        
        try:
            encoded_jwt = jwt.encode(to_encode, self.secret_key, algorithm=self.algorithm)
            return encoded_jwt
        except Exception as e:
            logger.error(f"Token creation failed: {e}")
            raise ValueError("Token creation failed")
    
    def create_refresh_token(self, data: Dict[str, Any]) -> str:
        """创建刷新令牌"""
        to_encode = data.copy()
        expire = datetime.utcnow() + timedelta(days=self.refresh_token_expire_days)
        
        to_encode.update({
            "exp": expire,
            "iat": datetime.utcnow(),
            "type": "refresh"
        })
        
        try:
            encoded_jwt = jwt.encode(to_encode, self.secret_key, algorithm=self.algorithm)
            return encoded_jwt
        except Exception as e:
            logger.error(f"Refresh token creation failed: {e}")
            raise ValueError("Refresh token creation failed")
    
    def verify_token(self, token: str) -> Optional[Dict[str, Any]]:
        """验证令牌"""
        try:
            payload = jwt.decode(token, self.secret_key, algorithms=[self.algorithm])
            
            # 检查令牌类型
            token_type = payload.get("type")
            if token_type not in ["access", "refresh"]:
                return None
            
            return payload
            
        except jwt.ExpiredSignatureError:
            logger.warning("Token has expired")
            return None
        except jwt.InvalidTokenError as e:
            logger.warning(f"Invalid token: {e}")
            return None
        except Exception as e:
            logger.error(f"Token verification failed: {e}")
            return None
    
    def decode_token(self, token: str) -> Optional[Dict[str, Any]]:
        """解码令牌（不验证过期时间）"""
        try:
            payload = jwt.decode(token, self.secret_key, algorithms=[self.algorithm], options={"verify_exp": False})
            return payload
        except Exception as e:
            logger.error(f"Token decoding failed: {e}")
            return None
    
    # ==================== API签名相关 ====================
    
    def create_api_signature(self, method: str, path: str, timestamp: str, body: str = "") -> str:
        """创建API签名（用于外部API调用）"""
        message = f"{method.upper()}{path}{timestamp}{body}"
        signature = hmac.new(
            self.secret_key.encode('utf-8'),
            message.encode('utf-8'),
            hashlib.sha256
        ).hexdigest()
        return signature
    
    def verify_api_signature(self, signature: str, method: str, path: str, timestamp: str, body: str = "") -> bool:
        """验证API签名"""
        expected_signature = self.create_api_signature(method, path, timestamp, body)
        return hmac.compare_digest(signature, expected_signature)
    
    # ==================== 数据加密相关 ====================
    
    def encrypt_sensitive_data(self, data: str) -> str:
        """加密敏感数据（简单实现，生产环境应使用更强的加密）"""
        # 这里使用简单的哈希，实际应用中应使用对称加密如AES
        return hashlib.sha256((data + self.secret_key).encode()).hexdigest()
    
    def generate_csrf_token(self) -> str:
        """生成CSRF令牌"""
        return secrets.token_urlsafe(32)
    
    def generate_api_key(self) -> str:
        """生成API密钥"""
        return secrets.token_urlsafe(48)

# 全局安全管理器实例
security_manager = SecurityManager()

# ==================== 认证相关异常 ====================

class AuthenticationError(HTTPException):
    """认证错误"""
    def __init__(self, detail: str = "Authentication failed"):
        super().__init__(status_code=status.HTTP_401_UNAUTHORIZED, detail=detail)

class AuthorizationError(HTTPException):
    """授权错误"""
    def __init__(self, detail: str = "Insufficient permissions"):
        super().__init__(status_code=status.HTTP_403_FORBIDDEN, detail=detail)

class TokenExpiredError(AuthenticationError):
    """令牌过期错误"""
    def __init__(self):
        super().__init__(detail="Token has expired")

class InvalidTokenError(AuthenticationError):
    """无效令牌错误"""
    def __init__(self):
        super().__init__(detail="Invalid token")

# ==================== 依赖注入函数 ====================

async def get_current_user_from_token(
    credentials: Optional[HTTPAuthorizationCredentials] = Depends(security),
    db: Session = Depends(get_database_session)
):
    """从token获取当前用户"""
    if not credentials:
        raise AuthenticationError("Missing authentication token")
    
    token = credentials.credentials
    payload = security_manager.verify_token(token)
    
    if not payload:
        raise InvalidTokenError()
    
    if payload.get("type") != "access":
        raise InvalidTokenError()
    
    user_id = payload.get("sub")
    if not user_id:
        raise InvalidTokenError()
    
    # 这里需要从数据库获取用户
    # 暂时返回用户ID，在实际实现时需要查询用户表
    return {"user_id": int(user_id), "payload": payload}

def require_permissions(required_permissions: List[str]):
    """权限装饰器"""
    def decorator(current_user: dict = Depends(get_current_user_from_token)):
        # 检查用户权限
        user_permissions = current_user.get("permissions", [])
        
        for permission in required_permissions:
            if permission not in user_permissions:
                raise AuthorizationError(f"Missing required permission: {permission}")
        
        return current_user
    
    return decorator

def require_super_user():
    """超级用户装饰器"""
    def decorator(current_user: dict = Depends(get_current_user_from_token)):
        if not current_user.get("is_super", False):
            raise AuthorizationError("Super user access required")
        
        return current_user
    
    return decorator

# ==================== 工具函数 ====================

def create_user_token(user_id: int, email: str, permissions: List[str], is_super: bool = False) -> Dict[str, str]:
    """为用户创建令牌对"""
    token_data = {
        "sub": str(user_id),
        "email": email,
        "permissions": permissions,
        "is_super": is_super
    }
    
    access_token = security_manager.create_access_token(token_data)
    refresh_token = security_manager.create_refresh_token({"sub": str(user_id)})
    
    return {
        "access_token": access_token,
        "refresh_token": refresh_token,
        "token_type": "bearer"
    }

def validate_password(password: str) -> bool:
    """验证密码强度"""
    errors = security_manager.validate_password_strength(password)
    if errors:
        raise ValueError("; ".join(errors))
    return True

# ==================== 中间件相关 ====================

def create_security_headers() -> Dict[str, str]:
    """创建安全头"""
    return {
        "X-Content-Type-Options": "nosniff",
        "X-Frame-Options": "DENY",
        "X-XSS-Protection": "1; mode=block",
        "Strict-Transport-Security": "max-age=31536000; includeSubDomains",
        "Referrer-Policy": "strict-origin-when-cross-origin",
        "Permissions-Policy": "geolocation=(), microphone=(), camera=()"
    }

# 导出主要接口
__all__ = [
    'security_manager',
    'AuthenticationError',
    'AuthorizationError', 
    'TokenExpiredError',
    'InvalidTokenError',
    'get_current_user_from_token',
    'require_permissions',
    'require_super_user',
    'create_user_token',
    'validate_password',
    'create_security_headers'
]