"""
安全的生产环境配置管理
"""
from typing import Optional
import os
from pydantic import Field, validator
from pydantic_settings import BaseSettings


class SecureSettings(BaseSettings):
    """
    安全的配置类 - 所有敏感信息必须从环境变量读取
    """
    
    # ========== 环境配置 ==========
    environment: str = Field(default="development", env="ENVIRONMENT")
    
    # ========== MySQL数据库配置 ==========
    mysql_host: str = Field(..., env="MYSQL_HOST")
    mysql_port: int = Field(default=3306, env="MYSQL_PORT")
    mysql_user: str = Field(..., env="MYSQL_USER")
    mysql_password: str = Field(..., env="MYSQL_PASSWORD")
    mysql_db: str = Field(..., env="MYSQL_DB")
    
    # 完整数据库URL（优先使用）
    database_url: Optional[str] = Field(None, env="DATABASE_URL")
    
    # ========== 安全密钥 ==========
    secret_key: str = Field(..., env="SECRET_KEY", min_length=32)
    jwt_secret_key: str = Field(..., env="JWT_SECRET_KEY", min_length=32)
    
    # ========== 用户管理 ==========
    default_user_password: str = Field(
        default="ChangeMe123!", 
        env="DEFAULT_USER_PASSWORD"
    )
    
    # ========== OneToken API配置 ==========
    onetoken_base_url: str = Field(
        default="https://stakestone.1token.tech/api/v1",
        env="ONETOKEN_BASE_URL"
    )
    onetoken_api_key: str = Field(..., env="ONETOKEN_API_KEY")
    onetoken_secret: str = Field(..., env="ONETOKEN_SECRET")
    onetoken_timeout: int = Field(default=30, env="ONETOKEN_TIMEOUT")
    
    # ========== Ceffu API配置 ==========
    ceffu_api_key: Optional[str] = Field(None, env="CEFFU_API_KEY")
    ceffu_public_key: Optional[str] = Field(None, env="CEFFU_PUBLIC_KEY")
    ceffu_private_key: Optional[str] = Field(None, env="CEFFU_PRIVATE_KEY")
    ceffu_secret_key: Optional[str] = Field(None, env="CEFFU_SECRET_KEY")
    ceffu_timeout: int = Field(default=30, env="CEFFU_TIMEOUT")
    
    # ========== Redis配置 ==========
    redis_host: str = Field(default="localhost", env="REDIS_HOST")
    redis_port: int = Field(default=6379, env="REDIS_PORT")
    redis_db: int = Field(default=0, env="REDIS_DB")
    redis_password: Optional[str] = Field(None, env="REDIS_PASSWORD")
    redis_url: Optional[str] = Field(None, env="REDIS_URL")
    
    # ========== 日志配置 ==========
    log_level: str = Field(default="INFO", env="LOG_LEVEL")
    log_file_path: str = Field(
        default="/var/log/fund_api/app.log",
        env="LOG_FILE_PATH"
    )
    
    # ========== 服务器配置 ==========
    server_host: str = Field(default="0.0.0.0", env="SERVER_HOST")
    server_port: int = Field(default=8000, env="SERVER_PORT")
    
    # ========== 数据更新间隔 ==========
    update_interval: int = Field(default=3600, env="UPDATE_INTERVAL")
    
    # ========== 数据库连接池配置 ==========
    db_pool_size: int = Field(default=20, env="DB_POOL_SIZE")
    db_max_overflow: int = Field(default=40, env="DB_MAX_OVERFLOW")
    db_pool_timeout: int = Field(default=30, env="DB_POOL_TIMEOUT")
    db_pool_recycle: int = Field(default=3600, env="DB_POOL_RECYCLE")
    
    # ========== 限流配置 ==========
    rate_limit_enabled: bool = Field(default=True, env="RATE_LIMIT_ENABLED")
    rate_limit_per_minute: int = Field(default=100, env="RATE_LIMIT_PER_MINUTE")
    rate_limit_per_hour: int = Field(default=1000, env="RATE_LIMIT_PER_HOUR")
    
    # ========== 监控配置 ==========
    sentry_dsn: Optional[str] = Field(None, env="SENTRY_DSN")
    
    class Config:
        env_file = ".env"
        env_file_encoding = "utf-8"
        case_sensitive = False
        extra = "ignore"
    
    @validator("secret_key", "jwt_secret_key")
    def validate_secret_keys(cls, v, field):
        """验证密钥强度"""
        if not v or len(v) < 32:
            raise ValueError(f"{field.name} must be at least 32 characters long")
        
        # 检查是否使用了默认值
        weak_patterns = [
            "your-super-secret-key",
            "change-in-production",
            "your_64_character",
            "change_this",
        ]
        
        for pattern in weak_patterns:
            if pattern in v.lower():
                raise ValueError(
                    f"{field.name} appears to be using a default/template value. "
                    "Please generate a strong random key."
                )
        
        return v
    
    @validator("mysql_password")
    def validate_db_password(cls, v):
        """验证数据库密码强度"""
        if not v:
            raise ValueError("Database password is required")
        
        if len(v) < 8:
            raise ValueError("Database password must be at least 8 characters long")
        
        # 生产环境额外检查
        if os.getenv("ENVIRONMENT") == "production":
            if v in ["123456", "password", "admin", "root"]:
                raise ValueError(
                    "Database password is too weak for production environment"
                )
        
        return v
    
    @validator("database_url", always=True)
    def build_database_url(cls, v, values):
        """构建数据库连接URL"""
        if v:
            return v
        
        # 从各个字段构建
        try:
            host = values.get("mysql_host")
            port = values.get("mysql_port", 3306)
            user = values.get("mysql_user")
            password = values.get("mysql_password")
            db = values.get("mysql_db")
            
            if all([host, user, password, db]):
                return f"mysql+pymysql://{user}:{password}@{host}:{port}/{db}?charset=utf8mb4"
        except Exception:
            pass
        
        return v
    
    @validator("redis_url", always=True)
    def build_redis_url(cls, v, values):
        """构建Redis连接URL"""
        if v:
            return v
        
        host = values.get("redis_host", "localhost")
        port = values.get("redis_port", 6379)
        db = values.get("redis_db", 0)
        password = values.get("redis_password")
        
        if password:
            return f"redis://:{password}@{host}:{port}/{db}"
        else:
            return f"redis://{host}:{port}/{db}"
    
    def get_database_url(self, hide_password: bool = False) -> str:
        """
        获取数据库连接URL
        
        Args:
            hide_password: 是否隐藏密码（用于日志）
        """
        url = self.database_url
        if hide_password and url:
            # 隐藏密码用于日志输出
            import re
            return re.sub(r"://([^:]+):([^@]+)@", r"://\1:***@", url)
        return url
    
    def validate_all_secrets(self) -> dict:
        """
        验证所有必需的密钥是否已设置
        返回缺失或无效的密钥列表
        """
        issues = {}
        
        # 检查必需的密钥
        required_keys = {
            "secret_key": self.secret_key,
            "jwt_secret_key": self.jwt_secret_key,
            "mysql_password": self.mysql_password,
            "onetoken_api_key": self.onetoken_api_key,
            "onetoken_secret": self.onetoken_secret,
        }
        
        for key_name, key_value in required_keys.items():
            if not key_value:
                issues[key_name] = "Missing or empty"
            elif len(key_value) < 8:
                issues[key_name] = "Too short (< 8 characters)"
        
        return issues


# 创建全局安全设置实例
def get_settings() -> SecureSettings:
    """
    获取配置实例
    
    在首次调用时验证所有必需的环境变量
    """
    try:
        settings = SecureSettings()
        
        # 生产环境进行额外验证
        if settings.environment == "production":
            issues = settings.validate_all_secrets()
            if issues:
                error_msg = "Configuration validation failed:\n"
                for key, issue in issues.items():
                    error_msg += f"  - {key}: {issue}\n"
                raise ValueError(error_msg)
        
        return settings
    
    except Exception as e:
        print(f"❌ Configuration Error: {e}")
        print("\n📋 Please check:")
        print("  1. .env file exists and is readable")
        print("  2. All required environment variables are set")
        print("  3. Refer to .env.template for required variables")
        raise


# 兼容旧代码的settings实例
settings = get_settings()
