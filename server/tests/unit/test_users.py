"""
Unit tests for user management endpoints
"""
import pytest
from fastapi.testclient import TestClient
from sqlalchemy.orm import Session


@pytest.mark.unit
class TestUserEndpoints:
    """用户管理接口测试"""
    
    def test_get_users_list(self, client: TestClient, admin_headers):
        """测试获取用户列表"""
        response = client.get(
            "/users",
            headers=admin_headers
        )
        
        assert response.status_code == 200
        data = response.json()
        
        # 检查StandardResponse格式
        assert "isOK" in data
        assert "data" in data
    
    
    def test_get_user_by_id(self, client: TestClient, sample_user, auth_headers):
        """测试根据ID获取用户"""
        response = client.get(
            f"/users/{sample_user.id}",
            headers=auth_headers
        )
        
        assert response.status_code == 200
        data = response.json()
        
        if data.get("isOK"):
            user_data = data["data"]
            assert user_data["email"] == sample_user.email
            assert user_data["name"] == sample_user.name
            # 确保不返回密码
            assert "password" not in user_data
            assert "hashed_password" not in user_data
    
    
    def test_get_nonexistent_user(self, client: TestClient, auth_headers):
        """测试获取不存在的用户"""
        response = client.get(
            "/users/99999",
            headers=auth_headers
        )
        
        assert response.status_code in [200, 404]
        data = response.json()
        
        if response.status_code == 200:
            assert data["isOK"] is False
    
    
    def test_update_user(self, client: TestClient, sample_user, auth_headers):
        """测试更新用户信息"""
        response = client.patch(
            f"/users/{sample_user.id}",
            headers=auth_headers,
            json={
                "name": "Updated Name",
                "phone": "13900139001"
            }
        )
        
        assert response.status_code == 200
        data = response.json()
        
        if data.get("isOK"):
            assert data["data"]["name"] == "Updated Name"
    
    
    def test_delete_user(self, client: TestClient, sample_user, admin_headers):
        """测试删除用户（软删除）"""
        response = client.delete(
            f"/users/{sample_user.id}",
            headers=admin_headers
        )
        
        assert response.status_code == 200
        data = response.json()
        
        # 应该是软删除，修改状态而非物理删除
        assert "isOK" in data


@pytest.mark.unit
class TestUserValidation:
    """用户数据验证测试"""
    
    def test_email_validation(self):
        """测试邮箱格式验证"""
        from pydantic import BaseModel, EmailStr, ValidationError
        
        class EmailTest(BaseModel):
            email: EmailStr
        
        # 有效邮箱
        valid_email = EmailTest(email="test@example.com")
        assert valid_email.email == "test@example.com"
        
        # 无效邮箱应该抛出异常
        with pytest.raises(ValidationError):
            EmailTest(email="invalid_email")
    
    
    def test_phone_validation(self):
        """测试手机号验证"""
        import re
        
        phone_pattern = r"^1[3-9]\d{9}$"
        
        # 有效手机号
        assert re.match(phone_pattern, "13800138000")
        assert re.match(phone_pattern, "18912345678")
        
        # 无效手机号
        assert not re.match(phone_pattern, "12345678901")
        assert not re.match(phone_pattern, "1380013800")
