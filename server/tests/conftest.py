"""
Pytest configuration and fixtures
"""
import pytest
import sys
from pathlib import Path
from typing import Generator
from sqlalchemy import create_engine
from sqlalchemy.orm import sessionmaker, Session
from sqlalchemy.pool import StaticPool
from fastapi.testclient import TestClient

# 添加项目根目录到路径
project_root = Path(__file__).parent.parent
sys.path.insert(0, str(project_root))

from app.main import app
from app.db.mysql import Base, get_db
from app.models.schemas import User, Portfolio, ProfitAllocationRatio
from app.settings import settings


# ============ 数据库测试配置 ============

# 使用内存SQLite数据库进行测试
SQLALCHEMY_TEST_DATABASE_URL = "sqlite:///:memory:"

@pytest.fixture(scope="function")
def test_engine():
    """创建测试数据库引擎"""
    engine = create_engine(
        SQLALCHEMY_TEST_DATABASE_URL,
        connect_args={"check_same_thread": False},
        poolclass=StaticPool,
    )
    # 创建所有表
    Base.metadata.create_all(bind=engine)
    yield engine
    # 测试后清理
    Base.metadata.drop_all(bind=engine)
    engine.dispose()


@pytest.fixture(scope="function")
def test_db(test_engine) -> Generator[Session, None, None]:
    """创建测试数据库会话"""
    TestingSessionLocal = sessionmaker(
        autocommit=False,
        autoflush=False,
        bind=test_engine
    )
    db = TestingSessionLocal()
    try:
        yield db
    finally:
        db.close()


@pytest.fixture(scope="function")
def client(test_db: Session) -> Generator[TestClient, None, None]:
    """创建测试客户端，注入测试数据库"""
    def override_get_db():
        try:
            yield test_db
        finally:
            pass
    
    app.dependency_overrides[get_db] = override_get_db
    
    with TestClient(app) as test_client:
        yield test_client
    
    app.dependency_overrides.clear()


# ============ 测试数据 Fixtures ============

@pytest.fixture
def sample_user(test_db: Session) -> User:
    """创建测试用户"""
    user = User(
        email="test@example.com",
        hashed_password="$2b$12$KIXxLVQZhG5qN9J3wP.8Ze3Z7qV9xQZpxH4K2vN3J5L8K9P7Q6M8a",  # password123
        name="Test User",
        phone="13800138000",
        role="user",
        status="active"
    )
    test_db.add(user)
    test_db.commit()
    test_db.refresh(user)
    return user


@pytest.fixture
def admin_user(test_db: Session) -> User:
    """创建测试管理员"""
    admin = User(
        email="admin@example.com",
        hashed_password="$2b$12$KIXxLVQZhG5qN9J3wP.8Ze3Z7qV9xQZpxH4K2vN3J5L8K9P7Q6M8a",
        name="Admin User",
        phone="13900139000",
        role="admin",
        status="active"
    )
    test_db.add(admin)
    test_db.commit()
    test_db.refresh(admin)
    return admin


@pytest.fixture
def sample_portfolio(test_db: Session, sample_user: User) -> Portfolio:
    """创建测试投资组合"""
    portfolio = Portfolio(
        code="TEST_PORTFOLIO_001",
        name="Test Portfolio",
        user_id=sample_user.id,
        total_equity=100000.00,
        available_to_withdraw=50000.00,
        status="active"
    )
    test_db.add(portfolio)
    test_db.commit()
    test_db.refresh(portfolio)
    return portfolio


@pytest.fixture
def auth_headers(sample_user: User) -> dict:
    """生成认证头部（使用JWT token生成）"""
    from server.app.jwt_utils import create_test_token
    token = create_test_token(
        user_id=sample_user.id,
        is_admin=False,
        permissions=["portfolio"]
    )
    return {"Authorization": f"Bearer {token}"}


@pytest.fixture
def admin_headers(admin_user: User) -> dict:
    """生成管理员认证头部"""
    from server.app.jwt_utils import create_admin_token
    token = create_admin_token()
    return {"Authorization": f"Bearer {token}"}


# ============ Mock外部服务 ============

@pytest.fixture
def mock_onetoken_client(monkeypatch):
    """Mock OneToken客户端"""
    class MockOneTokenClient:
        def get_portfolios(self):
            return [
                {
                    "portfolio": "mock_portfolio_1",
                    "name": "Mock Portfolio 1",
                    "total_equity": 100000.00
                }
            ]
        
        def get_portfolio_info(self, portfolio_code: str):
            return {
                "portfolio": portfolio_code,
                "total_equity": 100000.00,
                "available_to_withdraw": 50000.00
            }
    
    # 替换真实客户端
    # monkeypatch.setattr("app.services.external.onetoken_client", MockOneTokenClient())
    return MockOneTokenClient()


@pytest.fixture
def mock_ceffu_client(monkeypatch):
    """Mock Ceffu客户端"""
    class MockCeffuClient:
        def get_wallet_balance(self, wallet_id: str):
            return {
                "wallet_id": wallet_id,
                "balance": 50000.00,
                "currency": "USDT"
            }
        
        def create_withdrawal(self, wallet_id: str, amount: float, address: str):
            return {
                "withdrawal_id": "mock_withdrawal_123",
                "status": "pending",
                "amount": amount
            }
    
    return MockCeffuClient()


# ============ 测试辅助函数 ============

def assert_success_response(response_data: dict):
    """断言StandardResponse成功响应"""
    assert "isOK" in response_data
    assert response_data["isOK"] is True
    assert "data" in response_data


def assert_error_response(response_data: dict, expected_message: str = None):
    """断言StandardResponse错误响应"""
    assert "isOK" in response_data
    assert response_data["isOK"] is False
    assert "message" in response_data
    if expected_message:
        assert expected_message in response_data["message"]


def assert_pagination_response(response_data: dict, expected_total: int = None):
    """断言分页响应"""
    assert_success_response(response_data)
    assert "data" in response_data
    data = response_data["data"]
    assert "items" in data or "list" in data
    assert "total" in data
    if expected_total is not None:
        assert data["total"] == expected_total


# ============ 测试配置 ============

pytest_plugins = []

def pytest_configure(config):
    """Pytest配置"""
    config.addinivalue_line(
        "markers", "unit: mark test as a unit test"
    )
    config.addinivalue_line(
        "markers", "integration: mark test as an integration test"
    )
    config.addinivalue_line(
        "markers", "slow: mark test as slow running"
    )
    config.addinivalue_line(
        "markers", "auth: mark test as requiring authentication"
    )
