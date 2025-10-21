"""
系统状态相关模式
"""
from pydantic import BaseModel

class SystemStatusResponse(BaseModel):
    """系统状态响应模式"""
    watermark: int

    class Config:
        from_attributes = True