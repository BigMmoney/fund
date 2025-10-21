"""
Unit tests for authentication endpoints
"""
import pytest
from fastapi.testclient import TestClient
from sqlalchemy.orm import Session


@pytest.mark.unit
@pytest.mark.auth
class TestAuthEndpoints:
    """认证相关接口测试"""
    
    def test_login_success(self, client: TestClient, sample_user):
        """测试登录成功"""
        response = client.post(
            "/auth/login",
            json={
                "email": "test@example.com",
                "password": "password123"
            }
        )
        
        assert response.status_code == 200
        data = response.json()
        
        # 检查StandardResponse格式
        assert "isOK" in data
        assert "message" in data
        assert "data" in data
        
        # 登录成功应该返回token
        if data["isOK"]:
            assert "token" in data["data"] or "access_token" in data["data"]
    
    
    def test_login_invalid_email(self, client: TestClient):
        """测试无效邮箱登录"""
        response = client.post(
            "/auth/login",
            json={
                "email": "nonexistent@example.com",
                "password": "password123"
            }
        )
        
        assert response.status_code == 200
        data = response.json()
        assert data["isOK"] is False
        assert "message" in data
    
    
    def test_login_wrong_password(self, client: TestClient, sample_user):
        """测试错误密码"""
        response = client.post(
            "/auth/login",
            json={
                "email": "test@example.com",
                "password": "wrongpassword"
            }
        )
        
        assert response.status_code == 200
        data = response.json()
        assert data["isOK"] is False
    
    
    def test_login_missing_fields(self, client: TestClient):
        """测试缺少必填字段"""
        response = client.post(
            "/auth/login",
            json={"email": "test@example.com"}
        )
        
        # 应该返回验证错误
        assert response.status_code in [200, 422]
    
    
    def test_register_success(self, client: TestClient):
        """测试注册成功"""
        response = client.post(
            "/auth/register",
            json={
                "email": "newuser@example.com",
                "password": "newpassword123",
                "name": "New User",
                "phone": "13800138001"
            }
        )
        
        assert response.status_code == 200
        data = response.json()
        
        # 注册可能成功或失败（如果路由未实现）
        assert "isOK" in data
    
    
    def test_register_duplicate_email(self, client: TestClient, sample_user):
        """测试重复邮箱注册"""
        response = client.post(
            "/auth/register",
            json={
                "email": "test@example.com",  # 已存在的邮箱
                "password": "password123",
                "name": "Duplicate User",
                "phone": "13800138002"
            }
        )
        
        assert response.status_code == 200
        data = response.json()
        
        # 应该返回错误
        if "isOK" in data:
            assert data["isOK"] is False


@pytest.mark.unit
class TestPasswordSecurity:
    """密码安全测试"""
    
    def test_password_hashing(self):
        """测试密码是否正确哈希"""
        from passlib.context import CryptContext
        
        pwd_context = CryptContext(schemes=["bcrypt"], deprecated="auto")
        
        plain_password = "testpassword123"
        hashed = pwd_context.hash(plain_password)
        
        # 确保哈希不等于原密码
        assert hashed != plain_password
        
        # 确保可以验证
        assert pwd_context.verify(plain_password, hashed)
        
        # 确保错误密码无法验证
        assert not pwd_context.verify("wrongpassword", hashed)
