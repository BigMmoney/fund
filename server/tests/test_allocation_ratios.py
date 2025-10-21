"""
分配比例功能单元测试
"""
import pytest
from server.app.schemas import AllocationRatioCreate, AllocationRatioUpdate


class TestAllocationRatioValidation:
    """测试 Pydantic 验证逻辑"""
    
    def test_provide_all_three_values_valid(self):
        """测试提供全部 3 个值 (有效)"""
        data = AllocationRatioCreate(
            portfolioId=1,
            toUser=50,
            toPlatform=30,
            toTeam=20
        )
        assert data.toUser == 50
        assert data.toPlatform == 30
        assert data.toTeam == 20
        assert data.portfolioId == 1
    
    def test_provide_all_three_values_invalid_sum(self):
        """测试提供全部 3 个值 (总和 ≠ 100)"""
        with pytest.raises(ValueError) as exc_info:
            AllocationRatioCreate(
                portfolioId=1,
                toUser=50,
                toPlatform=40,
                toTeam=20
            )
        assert "总和必须等于 100" in str(exc_info.value)
        assert "当前总和为 110" in str(exc_info.value)
    
    def test_provide_two_values_user_platform(self):
        """测试提供 2 个值: toUser + toPlatform (自动计算 toTeam)"""
        data = AllocationRatioCreate(
            portfolioId=1,
            toUser=50,
            toPlatform=30
        )
        assert data.toUser == 50
        assert data.toPlatform == 30
        assert data.toTeam == 20  # 自动计算
    
    def test_provide_two_values_user_team(self):
        """测试提供 2 个值: toUser + toTeam (自动计算 toPlatform)"""
        data = AllocationRatioCreate(
            portfolioId=1,
            toUser=50,
            toTeam=20
        )
        assert data.toUser == 50
        assert data.toPlatform == 30  # 自动计算
        assert data.toTeam == 20
    
    def test_provide_two_values_platform_team(self):
        """测试提供 2 个值: toPlatform + toTeam (自动计算 toUser)"""
        data = AllocationRatioCreate(
            portfolioId=1,
            toPlatform=30,
            toTeam=20
        )
        assert data.toUser == 50  # 自动计算
        assert data.toPlatform == 30
        assert data.toTeam == 20
    
    def test_provide_only_one_value(self):
        """测试只提供 1 个值 (应该报错)"""
        with pytest.raises(ValueError) as exc_info:
            AllocationRatioCreate(
                portfolioId=1,
                toUser=50
            )
        assert "至少需要提供 2 个分配比例值" in str(exc_info.value)
    
    def test_provide_no_values(self):
        """测试不提供任何值 (应该报错)"""
        with pytest.raises(ValueError) as exc_info:
            AllocationRatioCreate(
                portfolioId=1
            )
        assert "必须至少提供 2 个分配比例值" in str(exc_info.value)
    
    def test_out_of_range_value_over_100(self):
        """测试超出范围的值 (> 100)"""
        with pytest.raises(ValueError) as exc_info:
            AllocationRatioCreate(
                portfolioId=1,
                toUser=150,
                toPlatform=30
            )
        assert "必须在 0-100 之间" in str(exc_info.value)
    
    def test_out_of_range_value_negative(self):
        """测试负值"""
        with pytest.raises(ValueError) as exc_info:
            AllocationRatioCreate(
                portfolioId=1,
                toUser=-10,
                toPlatform=60
            )
        assert "必须在 0-100 之间" in str(exc_info.value)
    
    def test_calculated_value_out_of_range_over_100(self):
        """测试自动计算的值超出范围 (> 100)"""
        with pytest.raises(ValueError) as exc_info:
            AllocationRatioCreate(
                portfolioId=1,
                toUser=50,
                toPlatform=-30  # 将导致 toTeam = 180
            )
        assert "必须在 0-100 之间" in str(exc_info.value)
    
    def test_calculated_value_out_of_range_negative(self):
        """测试自动计算的值超出范围 (< 0)"""
        with pytest.raises(ValueError) as exc_info:
            AllocationRatioCreate(
                portfolioId=1,
                toUser=50,
                toPlatform=80  # 将导致 toTeam = -30
            )
        assert "超出范围" in str(exc_info.value)
    
    def test_zero_values_valid(self):
        """测试边界值: 0% 是有效的"""
        data = AllocationRatioCreate(
            portfolioId=1,
            toUser=100,
            toPlatform=0,
            toTeam=0
        )
        assert data.toUser == 100
        assert data.toPlatform == 0
        assert data.toTeam == 0
    
    def test_update_with_two_values(self):
        """测试更新时提供 2 个值"""
        data = AllocationRatioUpdate(
            toUser=60,
            toPlatform=25
        )
        assert data.toUser == 60
        assert data.toPlatform == 25
        assert data.toTeam == 15  # 自动计算
    
    def test_update_with_all_three_values(self):
        """测试更新时提供全部 3 个值"""
        data = AllocationRatioUpdate(
            toUser=60,
            toPlatform=25,
            toTeam=15
        )
        assert data.toUser == 60
        assert data.toPlatform == 25
        assert data.toTeam == 15
    
    def test_invalid_portfolio_id_zero(self):
        """测试无效的投资组合 ID (0)"""
        with pytest.raises(ValueError) as exc_info:
            AllocationRatioCreate(
                portfolioId=0,
                toUser=50,
                toPlatform=30
            )
        assert "投资组合 ID 必须大于 0" in str(exc_info.value)
    
    def test_invalid_portfolio_id_negative(self):
        """测试无效的投资组合 ID (负数)"""
        with pytest.raises(ValueError) as exc_info:
            AllocationRatioCreate(
                portfolioId=-1,
                toUser=50,
                toPlatform=30
            )
        assert "投资组合 ID 必须大于 0" in str(exc_info.value)
    
    def test_float_values_rejected(self):
        """测试浮点数被拒绝 (只接受整数)"""
        with pytest.raises(ValueError) as exc_info:
            AllocationRatioCreate(
                portfolioId=1,
                toUser=50.5,
                toPlatform=30
            )
        assert "分配比例必须是整数" in str(exc_info.value)
    
    def test_extreme_valid_case_100_0_0(self):
        """测试极端情况: 100-0-0"""
        data = AllocationRatioCreate(
            portfolioId=1,
            toUser=100,
            toPlatform=0
        )
        assert data.toUser == 100
        assert data.toPlatform == 0
        assert data.toTeam == 0
    
    def test_extreme_valid_case_0_100_0(self):
        """测试极端情况: 0-100-0"""
        data = AllocationRatioCreate(
            portfolioId=1,
            toUser=0,
            toPlatform=100
        )
        assert data.toUser == 0
        assert data.toPlatform == 100
        assert data.toTeam == 0
    
    def test_extreme_valid_case_0_0_100(self):
        """测试极端情况: 0-0-100"""
        data = AllocationRatioCreate(
            portfolioId=1,
            toUser=0,
            toTeam=100
        )
        assert data.toUser == 0
        assert data.toPlatform == 0
        assert data.toTeam == 100
    
    def test_common_case_equal_split(self):
        """测试常见情况: 平均分配 (无法精确三等分)"""
        data = AllocationRatioCreate(
            portfolioId=1,
            toUser=33,
            toPlatform=33
        )
        assert data.toUser == 33
        assert data.toPlatform == 33
        assert data.toTeam == 34  # 34，而不是 33.33
    
    def test_common_case_user_70_platform_20(self):
        """测试常见情况: 用户 70%, 平台 20%, 团队 10%"""
        data = AllocationRatioCreate(
            portfolioId=1,
            toUser=70,
            toPlatform=20
        )
        assert data.toUser == 70
        assert data.toPlatform == 20
        assert data.toTeam == 10


class TestAllocationRatioEdgeCases:
    """测试边界情况"""
    
    def test_sum_99_should_fail(self):
        """测试总和为 99 (应该失败)"""
        with pytest.raises(ValueError):
            AllocationRatioCreate(
                portfolioId=1,
                toUser=50,
                toPlatform=30,
                toTeam=19
            )
    
    def test_sum_101_should_fail(self):
        """测试总和为 101 (应该失败)"""
        with pytest.raises(ValueError):
            AllocationRatioCreate(
                portfolioId=1,
                toUser=50,
                toPlatform=30,
                toTeam=21
            )
    
    def test_large_portfolio_id(self):
        """测试大的投资组合 ID"""
        data = AllocationRatioCreate(
            portfolioId=999999999,
            toUser=50,
            toPlatform=30
        )
        assert data.portfolioId == 999999999
        assert data.toTeam == 20


if __name__ == "__main__":
    # 运行测试
    pytest.main([__file__, "-v", "-s"])
