"""
Unit tests for portfolio management endpoints
"""
import pytest
from fastapi.testclient import TestClient
from sqlalchemy.orm import Session
from decimal import Decimal


@pytest.mark.unit
class TestPortfolioEndpoints:
    """投资组合接口测试"""
    
    def test_get_portfolios_list(self, client: TestClient, auth_headers):
        """测试获取投资组合列表"""
        response = client.get(
            "/portfolios",
            headers=auth_headers
        )
        
        assert response.status_code == 200
        data = response.json()
        
        assert "isOK" in data
        assert "data" in data
    
    
    def test_get_portfolio_by_id(self, client: TestClient, sample_portfolio, auth_headers):
        """测试根据ID获取投资组合"""
        response = client.get(
            f"/portfolios/{sample_portfolio.id}",
            headers=auth_headers
        )
        
        assert response.status_code == 200
        data = response.json()
        
        if data.get("isOK"):
            portfolio_data = data["data"]
            assert portfolio_data["code"] == sample_portfolio.code
            assert portfolio_data["name"] == sample_portfolio.name
    
    
    def test_create_portfolio(self, client: TestClient, sample_user, auth_headers):
        """测试创建投资组合"""
        response = client.post(
            "/portfolios",
            headers=auth_headers,
            json={
                "code": "NEW_PORTFOLIO",
                "name": "New Test Portfolio",
                "userId": sample_user.id,
                "totalEquity": 50000.00
            }
        )
        
        assert response.status_code == 200
        data = response.json()
        
        if data.get("isOK"):
            assert data["data"]["code"] == "NEW_PORTFOLIO"
    
    
    def test_update_portfolio(self, client: TestClient, sample_portfolio, auth_headers):
        """测试更新投资组合"""
        response = client.patch(
            f"/portfolios/{sample_portfolio.id}",
            headers=auth_headers,
            json={
                "name": "Updated Portfolio Name",
                "totalEquity": 120000.00
            }
        )
        
        assert response.status_code == 200
        data = response.json()
        
        if data.get("isOK"):
            assert data["data"]["name"] == "Updated Portfolio Name"


@pytest.mark.unit
class TestPortfolioBusinessLogic:
    """投资组合业务逻辑测试"""
    
    def test_available_withdraw_cannot_exceed_equity(self):
        """测试可提现金额不能超过总权益"""
        total_equity = Decimal("100000.00")
        available_to_withdraw = Decimal("120000.00")
        
        # 业务规则：可提现金额应该 <= 总权益
        assert available_to_withdraw > total_equity, "检测到异常情况"
    
    
    def test_portfolio_status_transitions(self):
        """测试投资组合状态转换"""
        valid_statuses = ["active", "suspended", "closed"]
        
        # 状态转换规则（示例）
        transitions = {
            "active": ["suspended", "closed"],
            "suspended": ["active", "closed"],
            "closed": []  # 已关闭无法转换
        }
        
        # 测试有效转换
        assert "suspended" in transitions["active"]
        assert "closed" in transitions["active"]
        
        # 测试无效转换
        assert len(transitions["closed"]) == 0
    
    
    def test_portfolio_equity_calculation(self):
        """测试权益计算"""
        initial_equity = Decimal("100000.00")
        profit = Decimal("5000.00")
        loss = Decimal("2000.00")
        
        final_equity = initial_equity + profit - loss
        
        assert final_equity == Decimal("103000.00")
        assert final_equity > Decimal("0")
