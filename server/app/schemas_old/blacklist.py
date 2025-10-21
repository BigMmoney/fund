"""
黑名单相关模式
"""
from pydantic import BaseModel, validator

class BlacklistCreate(BaseModel):
    """创建黑名单模式"""
    address: str
    note: str
    
    @validator('address')
    def validate_address(cls, v):
        """地址转换为小写"""
        return v.lower().strip()

class BlacklistResponse(BaseModel):
    """黑名单响应模式"""
    id: int
    address: str
    note: str
    created_at: int

    class Config:
        from_attributes = True