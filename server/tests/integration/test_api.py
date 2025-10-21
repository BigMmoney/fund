"""
Integration tests for API endpoints
"""
import pytest
from fastapi.testclient import TestClient


@pytest.mark.integration
class TestHealthCheck:
    """健康检查集成测试"""
    
    def test_health_endpoint(self, client: TestClient):
        """测试健康检查端点"""
        response = client.get("/health")
        
        assert response.status_code == 200
        data = response.json()
        
        # 检查健康检查响应
        assert "status" in data or "isOK" in data


@pytest.mark.integration
class TestDatabaseIntegration:
    """数据库集成测试"""
    
    def test_database_connection(self, test_db):
        """测试数据库连接"""
        from app.models.schemas import User
        
        # 尝试查询
        result = test_db.query(User).first()
        
        # 即使没有数据，查询也应该成功
        assert result is None or isinstance(result, User)
    
    
    def test_create_and_query_user(self, test_db):
        """测试创建和查询用户"""
        from app.models.schemas import User
        
        # 创建用户
        user = User(
            email="integration@test.com",
            hashed_password="hashed_pwd",
            name="Integration Test User",
            role="user",
            status="active"
        )
        test_db.add(user)
        test_db.commit()
        test_db.refresh(user)
        
        # 查询用户
        found_user = test_db.query(User).filter(User.email == "integration@test.com").first()
        
        assert found_user is not None
        assert found_user.email == "integration@test.com"
        assert found_user.name == "Integration Test User"


@pytest.mark.integration
@pytest.mark.slow
class TestExternalAPIIntegration:
    """外部API集成测试（需要Mock或实际环境）"""
    
    @pytest.mark.skip(reason="需要实际OneToken API凭证")
    def test_onetoken_connection(self):
        """测试OneToken API连接"""
        # TODO: 实现OneToken API连接测试
        pass
    
    
    @pytest.mark.skip(reason="需要实际Ceffu API凭证")
    def test_ceffu_connection(self):
        """测试Ceffu API连接"""
        # TODO: 实现Ceffu API连接测试
        pass
