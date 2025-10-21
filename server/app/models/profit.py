"""
收益相关模型
"""
from sqlalchemy import Column, Integer, String, Numeric, BigInteger, ForeignKey, Text
from sqlalchemy.orm import relationship
from .base import BaseModel

class AccProfitFromPortfolio(BaseModel):
    """投资组合累计收益快照"""
    __tablename__ = "acc_profit_from_portfolio"
    
    portfolio_id = Column(Integer, ForeignKey("portfolios.id"), nullable=False, comment="投资组合ID")
    snapshot_at = Column(BigInteger, nullable=False, index=True, comment="整时快照秒级时间?")
    acc_profit = Column(Numeric(20, 8), nullable=False, comment="累计收益")
    
    # 关系
    portfolio = relationship("Portfolio", back_populates="acc_profits")

class ProfitAllocationRatio(BaseModel):
    """收益分配比例"""
    __tablename__ = "profit_allocation_ratios"
    
    portfolio_id = Column(Integer, ForeignKey("portfolios.id"), nullable=False, comment="投资组合ID")
    version = Column(Integer, nullable=False, comment="版本?")
    to_team_ratio = Column(Integer, nullable=False, comment="团队分配比例?0000=100%")
    to_platform_ratio = Column(Integer, nullable=False, comment="平台分配比例?0000=100%")
    to_user_ratio = Column(Integer, nullable=False, comment="用户分配比例?0000=100%")
    created_by = Column(Integer, ForeignKey("users.id"), nullable=False, comment="创建者用户ID")
    
    # 关系
    portfolio = relationship("Portfolio", back_populates="profit_allocations")
    creator = relationship("User")

class ProfitAllocationLog(BaseModel):
    """收益分配日志"""
    __tablename__ = "profit_allocation_logs"
    
    portfolio_id = Column(Integer, ForeignKey("portfolios.id"), nullable=False, comment="投资组合ID")
    hour_end_at = Column(BigInteger, nullable=False, index=True, comment="整时结算时刻时间?")
    
    # 快照引用
    hourly_snapshot_prev_id = Column(Integer, ForeignKey("acc_profit_from_portfolio.id"), nullable=False)
    hourly_snapshot_curr_id = Column(Integer, ForeignKey("acc_profit_from_portfolio.id"), nullable=False)
    
    # 收益计算
    hourly_profit = Column(Numeric(20, 8), nullable=False, comment="一小时收益")
    profit_to_team = Column(Numeric(20, 8), nullable=False, comment="分配给团队的收益")
    profit_to_user = Column(Numeric(20, 8), nullable=False, comment="分配给用户的收益")
    profit_to_platform = Column(Numeric(20, 8), nullable=False, comment="分配给平台的收益")
    
    # 分配依据
    allocation_ratio_id = Column(Integer, ForeignKey("profit_allocation_ratios.id"), nullable=False)
    
    # 关系
    portfolio = relationship("Portfolio", back_populates="profit_logs")
    allocation_ratio = relationship("ProfitAllocationRatio")
    prev_snapshot = relationship("AccProfitFromPortfolio", foreign_keys=[hourly_snapshot_prev_id])
    curr_snapshot = relationship("AccProfitFromPortfolio", foreign_keys=[hourly_snapshot_curr_id])

class ProfitReallocation(BaseModel):
    """虚拟账户调账记录"""
    __tablename__ = "profit_reallocations"
    
    from_type = Column(String(50), nullable=False, comment="转出账户类型")
    to_type = Column(String(50), nullable=False, comment="转入账户类型")
    from_portfolio_id = Column(Integer, ForeignKey("portfolios.id"), nullable=True, comment="转出团队投组账户")
    to_portfolio_id = Column(Integer, ForeignKey("portfolios.id"), nullable=True, comment="转入团队投组账户")
    usd_value = Column(Numeric(20, 8), nullable=False, comment="调账金额USD")
    reason = Column(Text, nullable=False, comment="调账原因")
    created_by = Column(Integer, ForeignKey("users.id"), nullable=False, comment="创建者用户ID")
    
    # 关系
    from_portfolio = relationship("Portfolio", foreign_keys=[from_portfolio_id])
    to_portfolio = relationship("Portfolio", foreign_keys=[to_portfolio_id])
    creator = relationship("User")

class ProfitWithdrawal(BaseModel):
    """虚拟账户提现记录"""
    __tablename__ = "profit_withdrawals"
    
    from_type = Column(String(50), nullable=False, comment="提取类型")
    portfolio_id = Column(Integer, ForeignKey("portfolios.id"), nullable=True, comment="投资组合ID")
    
    # 区块链交易信?
    chain_id = Column(String(100), nullable=False, comment="链ID")
    transaction_hash = Column(String(255), nullable=False, comment="交易哈希")
    transaction_time = Column(BigInteger, nullable=False, comment="交易时间?")
    
    # 资产信息
    usd_value = Column(Numeric(20, 8), nullable=False, comment="USD价?")
    assets = Column(String(20), nullable=False, comment="代币种类")
    assets_amount = Column(Numeric(20, 8), nullable=False, comment="代币数量")
    
    created_by = Column(Integer, ForeignKey("users.id"), nullable=False, comment="创建者用户ID")
    
    # 关系
    portfolio = relationship("Portfolio")
    creator = relationship("User")

class HourlyProfitUser(BaseModel):
    """用户小时收益变动"""
    __tablename__ = "hourly_profit_user"
    
    hour_end_at = Column(BigInteger, nullable=False, index=True, comment="小时结束时间?")
    profit_delta = Column(Numeric(20, 8), nullable=False, comment="收益变动")
    delta_from_fund = Column(Numeric(20, 8), nullable=False, comment="基金收益变动")
    delta_from_reallocation = Column(Numeric(20, 8), nullable=False, comment="调账变动")

class HourlyProfitPlatform(BaseModel):
    """平台小时收益变动"""
    __tablename__ = "hourly_profit_platform"
    
    hour_end_at = Column(BigInteger, nullable=False, index=True, comment="小时结束时间?")
    profit_delta = Column(Numeric(20, 8), nullable=False, comment="收益变动")
    delta_from_fund = Column(Numeric(20, 8), nullable=False, comment="基金收益变动")
    delta_from_reallocation = Column(Numeric(20, 8), nullable=False, comment="调账变动")
    delta_from_withdraw = Column(Numeric(20, 8), nullable=False, comment="提现变动")

class HourlyProfitTeam(BaseModel):
    """团队小时收益变动"""
    __tablename__ = "hourly_profit_team"
    
    portfolio_id = Column(Integer, ForeignKey("portfolios.id"), nullable=False, comment="投资组合ID")
    hour_end_at = Column(BigInteger, nullable=False, index=True, comment="小时结束时间?")
    profit_delta = Column(Numeric(20, 8), nullable=False, comment="收益变动")
    delta_from_fund = Column(Numeric(20, 8), nullable=False, comment="基金收益变动")
    delta_from_reallocation = Column(Numeric(20, 8), nullable=False, comment="调账变动")
    delta_from_withdraw = Column(Numeric(20, 8), nullable=False, comment="提现变动")
    
    # 关系
    portfolio = relationship("Portfolio")

class AccProfitUser(BaseModel):
    """用户累计收益快照"""
    __tablename__ = "acc_profit_user"
    
    snapshot_at = Column(BigInteger, nullable=False, index=True, comment="快照时间?")
    acc_profit = Column(Numeric(20, 8), nullable=False, comment="累计收益")

class AccProfitPlatform(BaseModel):
    """平台累计收益快照"""
    __tablename__ = "acc_profit_platform"
    
    snapshot_at = Column(BigInteger, nullable=False, index=True, comment="快照时间?")
    acc_profit = Column(Numeric(20, 8), nullable=False, comment="累计收益")

class AccProfitTeam(BaseModel):
    """团队累计收益快照"""
    __tablename__ = "acc_profit_team"
    
    portfolio_id = Column(Integer, ForeignKey("portfolios.id"), nullable=False, comment="投资组合ID")
    snapshot_at = Column(BigInteger, nullable=False, index=True, comment="快照时间?")
    acc_profit = Column(Numeric(20, 8), nullable=False, comment="累计收益")
    
    # 关系
    portfolio = relationship("Portfolio")
