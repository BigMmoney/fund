"""
FastAPI Application Configuration
"""
from pydantic_settings import BaseSettings
from typing import List
import os


class Settings(BaseSettings):
    # Database Configuration
    # MySQL Configuration (for production)
    mysql_host: str = "cedefi-database-instance.cwwyatalynow.ap-northeast-1.rds.amazonaws.com"
    mysql_port: int = 49123
    mysql_username: str = "admin"
    mysql_password: str = "Cedefi2024"
    mysql_database: str = "fund_management"
    
    @property
    def database_url(self) -> str:
        """Construct database URL from MySQL settings"""
        return f"mysql+pymysql://{self.mysql_username}:{self.mysql_password}@{self.mysql_host}:{self.mysql_port}/{self.mysql_database}?charset=utf8mb4"
    
    # Security
    secret_key: str = "your-super-secret-key-change-in-production"
    algorithm: str = "HS256"
    access_token_expire_minutes: int = 1440  # 24 hours
    
    # CORS
    cors_origins: List[str] = ["http://localhost:3000", "http://localhost:8080"]
    allowed_origins: str = "*"
    
    # OneToken API Configuration
    onetoken_api_key: str = os.getenv("ONETOKEN_API_KEY", "your_onetoken_key_here")
    onetoken_secret: str = os.getenv("ONETOKEN_API_SECRET", "your_onetoken_secret_here")
    
    # Ceffu API Configuration
    ceffu_public_key: str = os.getenv("CEFFU_API_KEY", "OKbN7qJ6k3bnw15j")
    ceffu_private_key: str = os.getenv("CEFFU_API_SECRET", "your_ceffu_secret_here")
    ceffu_api_key: str = "OKbN7qJ6k3bnw15j"  # Legacy field for compatibility
    ceffu_api_url: str = "https://open-api.ceffu.com"
    ceffu_base_url: str = "https://open-api.ceffu.com"
    ceffu_secret_key: str = "your-ceffu-secret-key"
    ceffu_private_key_path: str = "/tmp/private_key.pem"
    
    # Prime Wallet Configuration (discovered IDs)
    zerodivision_btc_wallet_id: str = "540254796486533120"
    ci_usdt_zerod_bnb_wallet_id: str = "542168616511160320"
    
    # Redis
    redis_url: str = "redis://localhost:6379/0"
    
    # Application
    app_name: str = "OneToken Fund Management API"
    app_version: str = "1.0.0"
    debug: bool = True
    testing: bool = False
    
    # Logging
    log_level: str = "INFO"
    log_file: str = "/var/log/onetoken/api.log"
    
    class Config:
        env_file = ".env"
        case_sensitive = False
        extra = "ignore"  # Ignore extra fields from environment


# Global settings instance
settings = Settings()