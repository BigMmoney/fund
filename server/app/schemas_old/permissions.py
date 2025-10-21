"""
权限相关模式
"""
from pydantic import BaseModel

class PermissionResponse(BaseModel):
    """权限响应模式"""
    id: str
    label: str
    description: str

    class Config:
        from_attributes = True