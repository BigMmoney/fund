"""
投资组合模型
"""
from sqlalchemy import Column, Integer, String, ForeignKey, BigInteger
from sqlalchemy.orm import relationship
from .base import BaseModel

class Portfolio(BaseModel):
    """投资组合模型"""
    __tablename__ = "portfolios"
    
    # OneToken投资组合信息
    fund_name = Column(String(255), nullable=False, unique=True, comment="1token系统中投组的标识")
    fund_alias = Column(String(255), nullable=False, comment="1token系统中投组的显示名称")
    inception_time = Column(BigInteger, nullable=False, comment="1token系统中投组的创始时间?")
    
    # Binance子账号信?
    account_name = Column(String(255), nullable=False, comment="binance子账号在1token中的name")
    account_alias = Column(String(255), nullable=False, comment="binance子账号在1token中的别名")
    
    # Ceffu钱包信息
    ceffu_wallet_id = Column(String(255), nullable=False, comment="ceffu wallet 数字id")
    ceffu_wallet_name = Column(String(255), nullable=False, comment="ceffu wallet 显示名称")
    
    # 团队关联
    team_id = Column(Integer, ForeignKey("teams.id"), nullable=True, comment="所属的trade team id")
    
    # 上级投资组合（支持层级结构）
    parent_id = Column(Integer, ForeignKey("portfolios.id"), nullable=True, comment="上级投资组合")
    
    # 关系
    team = relationship("Team", back_populates="portfolios")
    # 注意：parent/children自关联关系在类定义之后设置，避免引用未定义的类属性
    # children 在类之后通过 backref 建立
    
    # 收益相关的关?
    profit_allocations = relationship("ProfitAllocationRatio", back_populates="portfolio")
    profit_logs = relationship("ProfitAllocationLog", back_populates="portfolio")
    acc_profits = relationship("AccProfitFromPortfolio", back_populates="portfolio")

# 在类定义之后设置自关联关系，确保remote_side正确指向本类的主键
Portfolio.parent = relationship(
    "Portfolio",
    remote_side=lambda: [Portfolio.id],
    backref="children",
)
