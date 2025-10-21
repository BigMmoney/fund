"""
User management router - 基于需求文档API [06-11]
用户管理系统，包括权限管理和禁用功能
"""
from typing import List, Optional
from fastapi import APIRouter, Depends, HTTPException, status, Query
from sqlalchemy.orm import Session
from sqlalchemy import or_

from server.app.database import get_db
from server.app.schemas import (
    UserCreate, UserUpdate, UserResponse, BaseResponse,
    PaginationParams, ListResponse, PaginationResponse
)
from server.app.models import User
from server.app.auth import get_password_hash
from server.app.api.dependencies import require_super_admin, require_user_permission
from server.app.config import settings

router = APIRouter(prefix="/users", tags=["User Management"])


# API [06] GET /users - 修改为返回全部用户，无需分页
@router.get("")
async def list_users(
    db: Session = Depends(get_db),
    current_user: User = Depends(require_user_permission)
):
    """获取用户的列表以及每个用户的权限，返回全部用户，无需分页"""
    from app.responses import StandardResponse
    from app.auth import AuthService
    
    try:
        # 获取所有用户
        users = db.query(User).all()
        
        # 格式化用户数据，包含权限信息
        user_list = []
        for user in users:
            # 获取用户权限
            permissions = AuthService.get_user_permissions(db, user.id)
            
            user_data = {
                "id": user.id,
                "isSuper": user.is_super,
                "email": user.email,
                "permissions": permissions,
                "suspended": getattr(user, 'suspended', False),
                "updatedAt": int(user.updated_at.timestamp()) if user.updated_at else None,
                "createdAt": int(user.created_at.timestamp()) if user.created_at else None
            }
            user_list.append(user_data)
        
        return StandardResponse.list_success(user_list, len(user_list))
        
    except Exception as e:
        return StandardResponse.error(f"Failed to get users: {str(e)}")


@router.get("/{user_id}", response_model=UserResponse)
async def get_user(
    user_id: int,
    db: Session = Depends(get_db),
    current_user: User = Depends(require_user_permission)
):
    """Get user by ID"""
    user = db.query(User).filter(User.id == user_id).first()
    if not user:
        raise HTTPException(status_code=404, detail="User not found")
    
    return user


@router.post("", response_model=UserResponse)
async def create_user(
    user_data: UserCreate,
    db: Session = Depends(get_db),
    current_user: User = Depends(require_super_admin)
):
    """Create a new user (super admin only)"""
    # Check if user already exists
    existing_user = db.query(User).filter(User.email == user_data.email).first()
    if existing_user:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Email already registered"
        )
    
    # Create new user
    hashed_password = get_password_hash(user_data.password)
    new_user = User(
        email=user_data.email,
        password_hash=hashed_password,
        name=user_data.name,
        is_super=user_data.is_super,
        is_active=user_data.is_active
    )
    
    db.add(new_user)
    db.commit()
    db.refresh(new_user)
    
    return new_user


@router.put("/{user_id}", response_model=UserResponse)
async def update_user(
    user_id: int,
    user_data: UserUpdate,
    db: Session = Depends(get_db),
    current_user: User = Depends(require_super_admin)
):
    """Update user (super admin only)"""
    user = db.query(User).filter(User.id == user_id).first()
    if not user:
        raise HTTPException(status_code=404, detail="User not found")
    
    # Check if email is already taken by another user
    if user_data.email and user_data.email != user.email:
        existing_user = db.query(User).filter(
            User.email == user_data.email,
            User.id != user_id
        ).first()
        if existing_user:
            raise HTTPException(
                status_code=status.HTTP_400_BAD_REQUEST,
                detail="Email already taken"
            )
    
    # Update fields
    update_data = user_data.dict(exclude_unset=True)
    for field, value in update_data.items():
        setattr(user, field, value)
    
    db.commit()
    db.refresh(user)
    
    return user


@router.patch("/{user_id}/activate", response_model=UserResponse)
async def activate_user(
    user_id: int,
    db: Session = Depends(get_db),
    current_user: User = Depends(require_super_admin)
):
    """Activate user account"""
    user = db.query(User).filter(User.id == user_id).first()
    if not user:
        raise HTTPException(status_code=404, detail="User not found")
    
    user.is_active = True
    db.commit()
    db.refresh(user)
    
    return user


@router.patch("/{user_id}/deactivate", response_model=UserResponse)
async def deactivate_user(
    user_id: int,
    db: Session = Depends(get_db),
    current_user: User = Depends(require_super_admin)
):
    """Deactivate user account"""
    user = db.query(User).filter(User.id == user_id).first()
    if not user:
        raise HTTPException(status_code=404, detail="User not found")
    
    # Prevent deactivating self
    if user.id == current_user.id:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Cannot deactivate your own account"
        )
    
    user.is_active = False
    db.commit()
    db.refresh(user)
    
    return user


# API [08] PATCH /users/{id}/reset
@router.patch("/{user_id}/reset")
async def reset_user_password(
    user_id: int,
    db: Session = Depends(get_db),
    current_user: User = Depends(require_user_permission)
):
    """管理员重新设置某个用户的密码，密码会被重置成初始密码"""
    from app.responses import StandardResponse
    from app.auth import AuthService
    
    try:
        # 查找用户
        user = db.query(User).filter(User.id == user_id).first()
        if not user:
            return StandardResponse.error("User not found")
        
        # 重置为默认密码
        default_password = settings.default_user_password or "123456"
        new_password_hash = AuthService.hash_password(default_password)
        
        user.password_hash = new_password_hash
        db.commit()
        
        return StandardResponse.success()
        
    except Exception as e:
        return StandardResponse.error(f"Password reset failed: {str(e)}")


# API [09] PATCH /users/{id}/permissions
@router.patch("/{user_id}/permissions")
async def update_user_permissions(
    user_id: int,
    permissions_data: dict,
    db: Session = Depends(get_db),
    current_user: User = Depends(require_user_permission)
):
    """修改某个用户的操作权限"""
    from app.responses import StandardResponse
    from app.models import UserPermission, Permission
    
    try:
        # 查找用户
        user = db.query(User).filter(User.id == user_id).first()
        if not user:
            return StandardResponse.error("User not found")
        
        # 验证权限数组
        permissions = permissions_data.get("permissions", [])
        if not isinstance(permissions, list):
            return StandardResponse.error("Permissions must be an array")
        
        # 验证权限是否存在
        valid_permissions = db.query(Permission.id).all()
        valid_permission_ids = {p.id for p in valid_permissions}
        
        for perm_id in permissions:
            if perm_id not in valid_permission_ids:
                return StandardResponse.error(f"Invalid permission: {perm_id}")
        
        # 删除现有权限
        db.query(UserPermission).filter(UserPermission.user_id == user_id).delete()
        
        # 添加新权限
        for perm_id in permissions:
            user_perm = UserPermission(user_id=user_id, permission_id=perm_id)
            db.add(user_perm)
        
        db.commit()
        
        # 刷新用户信息
        db.refresh(user)
        
        # 获取更新后的权限列表
        from server.app.auth import AuthService
        updated_permissions = AuthService.get_user_permissions(db, user_id)
        
        # 格式化用户数据
        user_data = AuthService.format_user_response(user, updated_permissions)
        
        return StandardResponse.object_success(user_data)
        
    except Exception as e:
        return StandardResponse.error(f"Permission update failed: {str(e)}")


# API [10] PATCH /users/{id}/suspend
@router.patch("/{user_id}/suspend")
async def suspend_user(
    user_id: int,
    db: Session = Depends(get_db),
    current_user: User = Depends(require_user_permission)
):
    """用户禁用功能 - 根据2025-09-24需求更新"""
    from app.responses import StandardResponse
    from app.auth import AuthService
    
    try:
        # 查找用户
        user = db.query(User).filter(User.id == user_id).first()
        if not user:
            return StandardResponse.error("User not found")
        
        # 防止禁用自己
        if user.id == current_user.id:
            return StandardResponse.error("Cannot suspend your own account")
        
        # 设置suspended状态
        user.suspended = True
        db.commit()
        db.refresh(user)
        
        # 获取用户权限
        permissions = AuthService.get_user_permissions(db, user_id)
        
        # 格式化用户数据
        user_data = AuthService.format_user_response(user, permissions)
        
        return StandardResponse.object_success(user_data)
        
    except Exception as e:
        return StandardResponse.error(f"User suspension failed: {str(e)}")