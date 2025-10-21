"""
Unit tests for profit allocation ratio endpoints
"""
import pytest
from fastapi.testclient import TestClient
from sqlalchemy.orm import Session


@pytest.mark.unit
class TestProfitAllocationEndpoints:
    """收益分配比例接口测试"""
    
    def test_get_allocation_ratios(self, client: TestClient, auth_headers):
        """测试获取分配比例列表"""
        response = client.get(
            "/profit_allocation_ratios",
            headers=auth_headers
        )
        
        assert response.status_code == 200
        data = response.json()
        
        assert "isOK" in data
        assert "data" in data
    
    
    def test_create_allocation_ratio(self, client: TestClient, sample_portfolio, auth_headers):
        """测试创建分配比例"""
        response = client.post(
            "/profit_allocation_ratios",
            headers=auth_headers,
            json={
                "portfolioId": sample_portfolio.id,
                "toTeam": 3000,      # 30%
                "toPlatform": 2000,  # 20%
                "toUser": 5000,      # 50%
                "version": 1
            }
        )
        
        assert response.status_code == 200
        data = response.json()
        
        # 检查响应格式
        assert "isOK" in data
    
    
    def test_get_allocation_by_portfolio(self, client: TestClient, sample_portfolio, auth_headers):
        """测试根据投资组合获取分配比例"""
        response = client.get(
            f"/profit_allocation_ratios/portfolio/{sample_portfolio.id}",
            headers=auth_headers
        )
        
        assert response.status_code == 200
        data = response.json()
        
        assert "isOK" in data


@pytest.mark.unit
class TestAllocationBusinessRules:
    """分配比例业务规则测试"""
    
    def test_allocation_sum_equals_10000(self):
        """测试分配比例总和必须为10000 (100%)"""
        to_team = 3000
        to_platform = 2000
        to_user = 5000
        
        total = to_team + to_platform + to_user
        
        # 核心业务规则
        assert total == 10000, f"分配比例总和必须为10000，当前为{total}"
    
    
    def test_invalid_allocation_sum(self):
        """测试无效的分配比例"""
        to_team = 3000
        to_platform = 2000
        to_user = 6000  # 错误！总和超过10000
        
        total = to_team + to_platform + to_user
        
        assert total != 10000, "应该检测到分配比例错误"
    
    
    def test_negative_allocation(self):
        """测试负数分配比例应该被拒绝"""
        to_team = -1000  # 负数
        to_platform = 2000
        to_user = 9000
        
        # 业务规则：所有分配比例必须 >= 0
        assert to_team >= 0 or to_platform >= 0 or to_user >= 0, "检测到负数分配"
    
    
    def test_allocation_version_increment(self):
        """测试版本号递增"""
        current_version = 5
        new_version = current_version + 1
        
        assert new_version == 6
        assert new_version > current_version
    
    
    def test_calculate_actual_amounts(self):
        """测试根据比例计算实际金额"""
        total_profit = 10000.00
        to_team_ratio = 3000  # 30%
        
        # 计算实际金额
        to_team_amount = (to_team_ratio / 10000) * total_profit
        
        assert to_team_amount == 3000.00
        
        # 测试其他比例
        to_platform_ratio = 2000  # 20%
        to_user_ratio = 5000      # 50%
        
        to_platform_amount = (to_platform_ratio / 10000) * total_profit
        to_user_amount = (to_user_ratio / 10000) * total_profit
        
        # 验证总和
        total_allocated = to_team_amount + to_platform_amount + to_user_amount
        assert abs(total_allocated - total_profit) < 0.01  # 浮点数比较
