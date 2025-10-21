"""
JWT Token工具 - 用于测试和开发
"""
from datetime import datetime, timedelta
from typing import Optional, Dict, Any
from jose import jwt, JWTError
import secrets

# 从环境变量或配置读取
SECRET_KEY = "your-secret-key-change-in-production"
ALGORITHM = "HS256"
ACCESS_TOKEN_EXPIRE_MINUTES = 1440  # 24 hours


def create_access_token(
    data: Dict[str, Any],
    expires_delta: Optional[timedelta] = None
) -> str:
    """
    创建JWT access token
    
    Args:
        data: 要编码的数据（通常包含sub, is_admin等）
        expires_delta: 过期时间增量，默认24小时
    
    Returns:
        str: JWT token字符串
    """
    to_encode = data.copy()
    
    if expires_delta:
        expire = datetime.utcnow() + expires_delta
    else:
        expire = datetime.utcnow() + timedelta(minutes=ACCESS_TOKEN_EXPIRE_MINUTES)
    
    to_encode.update({
        "exp": expire,
        "iat": datetime.utcnow(),
        "jti": secrets.token_urlsafe(16)  # JWT ID for token tracking
    })
    
    encoded_jwt = jwt.encode(to_encode, SECRET_KEY, algorithm=ALGORITHM)
    return encoded_jwt


def decode_access_token(token: str) -> Optional[Dict[str, Any]]:
    """
    解码JWT access token
    
    Args:
        token: JWT token字符串
    
    Returns:
        dict: 解码后的payload，失败返回None
    """
    try:
        payload = jwt.decode(token, SECRET_KEY, algorithms=[ALGORITHM])
        return payload
    except JWTError as e:
        print(f"Token decode error: {e}")
        return None


def create_test_token(
    user_id: int,
    is_admin: bool = False,
    permissions: Optional[list] = None,
    expires_minutes: int = 1440
) -> str:
    """
    创建测试用JWT token
    
    Args:
        user_id: 用户ID
        is_admin: 是否是管理员
        permissions: 权限列表
        expires_minutes: 过期时间（分钟）
    
    Returns:
        str: JWT token
    """
    data = {
        "sub": str(user_id),
        "is_admin": is_admin,
        "permissions": permissions or []
    }
    
    expires_delta = timedelta(minutes=expires_minutes)
    return create_access_token(data, expires_delta)


def create_admin_token(expires_minutes: int = 1440) -> str:
    """创建管理员token"""
    return create_test_token(
        user_id=1,
        is_admin=True,
        permissions=["user", "team", "profit", "portfolio", "blacklist"],
        expires_minutes=expires_minutes
    )


def create_regular_user_token(
    user_id: int = 2,
    permissions: Optional[list] = None,
    expires_minutes: int = 1440
) -> str:
    """创建普通用户token"""
    return create_test_token(
        user_id=user_id,
        is_admin=False,
        permissions=permissions or ["portfolio"],
        expires_minutes=expires_minutes
    )


# 用于测试的预生成token
TEST_ADMIN_TOKEN = None
TEST_USER_TOKEN = None


def get_test_admin_token() -> str:
    """获取测试管理员token（懒加载）"""
    global TEST_ADMIN_TOKEN
    if not TEST_ADMIN_TOKEN:
        TEST_ADMIN_TOKEN = create_admin_token()
    return TEST_ADMIN_TOKEN


def get_test_user_token() -> str:
    """获取测试普通用户token（懒加载）"""
    global TEST_USER_TOKEN
    if not TEST_USER_TOKEN:
        TEST_USER_TOKEN = create_regular_user_token()
    return TEST_USER_TOKEN


if __name__ == "__main__":
    # 测试JWT token生成
    print("=== JWT Token 测试 ===\n")
    
    # 1. 创建管理员token
    admin_token = create_admin_token()
    print(f"管理员Token: {admin_token[:50]}...")
    
    # 2. 解码验证
    payload = decode_access_token(admin_token)
    print(f"\n解码后payload:")
    print(f"  用户ID: {payload.get('sub')}")
    print(f"  是否管理员: {payload.get('is_admin')}")
    print(f"  权限列表: {payload.get('permissions')}")
    print(f"  过期时间: {datetime.fromtimestamp(payload.get('exp'))}")
    
    # 3. 创建普通用户token
    user_token = create_regular_user_token(user_id=100, permissions=["portfolio", "team"])
    print(f"\n普通用户Token: {user_token[:50]}...")
    
    payload2 = decode_access_token(user_token)
    print(f"\n解码后payload:")
    print(f"  用户ID: {payload2.get('sub')}")
    print(f"  是否管理员: {payload2.get('is_admin')}")
    print(f"  权限列表: {payload2.get('permissions')}")
    
    print("\n✅ JWT Token工具测试完成")
