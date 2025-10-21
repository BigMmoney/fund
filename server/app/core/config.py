"""
核心配置管理 - 集中化配置管理
支持环境变量、配置文件、默认值的优先级管理
"""
import os
from typing import Optional, List, Dict, Any
from pydantic_settings import BaseSettings, SettingsConfigDict
from pydantic import validator
from functools import lru_cache


class DatabaseSettings(BaseSettings):
    """数据库配置"""
    model_config = SettingsConfigDict(env_prefix="DB_", case_sensitive=True)
    
    # 数据库连接配置
    DB_HOST: str = "localhost"
    DB_PORT: int = 3306
    DB_USER: str = "root"
    DB_PASSWORD: str = ""
    DB_NAME: str = "fund_management"
    DB_CHARSET: str = "utf8mb4"
    
    # 连接池配置
    DB_POOL_SIZE: int = 10
    DB_MAX_OVERFLOW: int = 20
    DB_POOL_TIMEOUT: int = 30
    DB_POOL_RECYCLE: int = 3600
    
    # SQLAlchemy配置
    DB_ECHO: bool = False  # 生产环境设为False
    
    @property
    def database_url(self) -> str:
        """构建数据库连接URL"""
        return (
            f"mysql+pymysql://{self.DB_USER}:{self.DB_PASSWORD}"
            f"@{self.DB_HOST}:{self.DB_PORT}/{self.DB_NAME}"
            f"?charset={self.DB_CHARSET}"
        )


class SecuritySettings(BaseSettings):
    """安全配置"""
    model_config = SettingsConfigDict(env_prefix="SECURITY_", case_sensitive=True)
    
    # JWT配置
    SECRET_KEY: str = "your-secret-key-change-in-production"
    ALGORITHM: str = "HS256"
    ACCESS_TOKEN_EXPIRE_MINUTES: int = 30
    REFRESH_TOKEN_EXPIRE_DAYS: int = 7
    
    # 密码策略
    PASSWORD_MIN_LENGTH: int = 8
    PASSWORD_REQUIRE_UPPERCASE: bool = True
    PASSWORD_REQUIRE_LOWERCASE: bool = True
    PASSWORD_REQUIRE_DIGITS: bool = True
    PASSWORD_REQUIRE_SPECIAL: bool = False
    
    # CORS配置
    CORS_ORIGINS: List[str] = ["*"]
    
    # 安全头配置
    ENABLE_SECURITY_HEADERS: bool = True


class OneTokenSettings(BaseSettings):
    """OneToken API配置"""
    model_config = SettingsConfigDict(env_prefix="ONETOKEN_", case_sensitive=True)
    
    # OneToken配置
    ONETOKEN_OT_KEY: str = ""
    ONETOKEN_OT_SECRET: str = ""
    ONETOKEN_BASE_URL: str = "https://api.onetoken.trade"
    ONETOKEN_TIMEOUT: int = 30
    ONETOKEN_RETRY_TIMES: int = 3


class CeffuSettings(BaseSettings):
    """Ceffu API配置"""
    model_config = SettingsConfigDict(env_prefix="CEFFU_", case_sensitive=True)
    
    # Ceffu配置
    CEFFU_API_KEY: str = ""
    CEFFU_SECRET_KEY: str = ""
    CEFFU_BASE_URL: str = "https://api.ceffu.com"
    CEFFU_TIMEOUT: int = 30
    CEFFU_RETRY_TIMES: int = 3


class LoggingSettings(BaseSettings):
    """日志配置"""
    model_config = SettingsConfigDict(env_prefix="LOG_", case_sensitive=True)
    
    # 日志配置
    LEVEL: str = "INFO"
    FORMAT: str = "%(asctime)s - %(name)s - %(levelname)s - %(message)s"
    FILE_PATH: Optional[str] = None
    MAX_FILE_SIZE: int = 10 * 1024 * 1024  # 10MB
    BACKUP_COUNT: int = 5
    
    # 结构化日志
    USE_JSON_LOGGING: bool = False


class AppSettings(BaseSettings):
    """应用配置"""
    model_config = SettingsConfigDict(env_prefix="APP_", case_sensitive=True)
    
    # 应用基本信息
    APP_NAME: str = "Fund Management API"
    APP_VERSION: str = "1.0.0"
    APP_DESCRIPTION: str = "Fund Management System with OneToken and Ceffu Integration"
    
    # 服务配置
    HOST: str = "0.0.0.0"
    PORT: int = 8000
    WORKERS: int = 1
    
    # 运行环境
    ENVIRONMENT: str = "development"  # development, staging, production
    DEBUG: bool = True
    
    # API文档配置
    DOCS_URL: str = "/docs"
    REDOC_URL: str = "/redoc"
    
    # 监控配置
    ENABLE_METRICS: bool = True
    METRICS_PATH: str = "/metrics"
    
    @property
    def is_production(self) -> bool:
        return self.ENVIRONMENT.lower() == "production"
    
    @property
    def is_development(self) -> bool:
        return self.ENVIRONMENT.lower() == "development"
    
    @property
    def is_staging(self) -> bool:
        return self.ENVIRONMENT.lower() == "staging"


class Settings(BaseSettings):
    """主配置类 - 聚合所有配置"""
    model_config = SettingsConfigDict(case_sensitive=True)
    
    # 子配置实例
    app: AppSettings = AppSettings()
    database: DatabaseSettings = DatabaseSettings()
    security: SecuritySettings = SecuritySettings()
    onetoken: OneTokenSettings = OneTokenSettings()
    ceffu: CeffuSettings = CeffuSettings()
    logging: LoggingSettings = LoggingSettings()
    
    def get_config_dict(self) -> Dict[str, Any]:
        """获取所有配置的字典形式（隐藏敏感信息）"""
        config = {}
        
        for section_name, section in [
            ("app", self.app),
            ("database", self.database),
            ("security", self.security),
            ("onetoken", self.onetoken),
            ("ceffu", self.ceffu),
            ("logging", self.logging)
        ]:
            section_dict = {}
            for key, value in section.__dict__.items():
                if not key.startswith('_'):
                    # 隐藏敏感信息
                    if any(sensitive in key.lower() for sensitive in 
                          ['password', 'secret', 'key', 'token']):
                        section_dict[key] = "***HIDDEN***" if value else ""
                    else:
                        section_dict[key] = value
            config[section_name] = section_dict
        
        return config


@lru_cache()
def get_settings() -> Settings:
    """获取配置实例（缓存）"""
    return Settings()


# 全局配置实例
settings = get_settings()


def validate_config() -> List[str]:
    """验证配置有效性"""
    errors = []
    
    # 数据库配置验证
    if not settings.database.DB_HOST:
        errors.append("Database host is required")
    
    if not settings.database.DB_NAME:
        errors.append("Database name is required")
    
    # 安全配置验证
    if settings.app.is_production:
        if settings.security.SECRET_KEY == "your-secret-key-change-in-production":
            errors.append("SECRET_KEY must be changed in production")
        
        if not settings.database.DB_PASSWORD:
            errors.append("Database password is required in production")
        
        if len(settings.security.SECRET_KEY) < 32:
            errors.append("SECRET_KEY should be at least 32 characters in production")
    
    # OneToken配置验证
    if not settings.onetoken.ONETOKEN_OT_KEY and settings.app.is_production:
        errors.append("OneToken OT_KEY is required in production")
    
    # Ceffu配置验证
    if not settings.ceffu.CEFFU_API_KEY and settings.app.is_production:
        errors.append("Ceffu API_KEY is required in production")
    
    return errors


def get_db_url() -> str:
    """获取数据库连接URL"""
    return settings.database.database_url


# 在模块加载时验证配置
config_errors = validate_config()
if config_errors and settings.app.is_production:
    raise ValueError(f"Configuration errors: {'; '.join(config_errors)}")
elif config_errors:
    print(f"Configuration warnings: {'; '.join(config_errors)}")