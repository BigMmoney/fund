"""
Database Models using SQLAlchemy ORM
"""
from sqlalchemy import Column, Integer, BigInteger, String, Text, Boolean, DECIMAL, TIMESTAMP, ForeignKey, JSON, Enum
from sqlalchemy.ext.declarative import declarative_base
from sqlalchemy.orm import relationship
from sqlalchemy.sql import func
from datetime import datetime
import enum

Base = declarative_base()


# ==========================================
# 用户认证和权限管理
# ==========================================

class User(Base):
    __tablename__ = "users"
    
    id = Column(BigInteger, primary_key=True, autoincrement=True)
    email = Column(String(255), nullable=False, unique=True, comment="用户邮箱")
    password_hash = Column(String(255), nullable=False, comment="密码哈希")
    is_super = Column(Boolean, default=False, comment="是否超级管理员")
    is_active = Column(Boolean, default=True, comment="是否激活")
    suspended = Column(Boolean, default=False, comment="是否被禁用")
    created_at = Column(TIMESTAMP, default=func.current_timestamp())
    updated_at = Column(TIMESTAMP, default=func.current_timestamp(), onupdate=func.current_timestamp())
    
    # Relationships
    permissions = relationship("UserPermission", back_populates="user", cascade="all, delete-orphan")
    sessions = relationship("UserSession", back_populates="user", cascade="all, delete-orphan")


class Permission(Base):
    __tablename__ = "permissions"
    
    id = Column(String(50), primary_key=True, comment="权限标识符")
    label = Column(String(100), nullable=False, comment="权限显示名称")
    description = Column(Text, comment="权限描述")
    created_at = Column(TIMESTAMP, default=func.current_timestamp())
    
    # Relationships
    user_permissions = relationship("UserPermission", back_populates="permission")


class UserPermission(Base):
    __tablename__ = "user_permissions"
    
    id = Column(BigInteger, primary_key=True, autoincrement=True)
    user_id = Column(BigInteger, ForeignKey("users.id", ondelete="CASCADE"), nullable=False)
    permission_id = Column(String(50), ForeignKey("permissions.id", ondelete="CASCADE"), nullable=False)
    created_at = Column(TIMESTAMP, default=func.current_timestamp())
    
    # Relationships
    user = relationship("User", back_populates="permissions")
    permission = relationship("Permission", back_populates="user_permissions")


class UserSession(Base):
    __tablename__ = "user_sessions"
    
    id = Column(BigInteger, primary_key=True, autoincrement=True)
    user_id = Column(BigInteger, ForeignKey("users.id", ondelete="CASCADE"), nullable=False)
    token = Column(String(500), nullable=False, unique=True, comment="JWT Token")
    expires_at = Column(TIMESTAMP, nullable=False, comment="过期时间")
    created_at = Column(TIMESTAMP, default=func.current_timestamp())
    
    # Relationships
    user = relationship("User", back_populates="sessions")


# ==========================================
# 交易团队管理
# ==========================================

class Team(Base):
    __tablename__ = "teams"
    
    id = Column(BigInteger, primary_key=True, autoincrement=True)
    name = Column(String(255), nullable=False, unique=True, comment="团队名称")
    description = Column(Text, comment="团队描述")
    is_active = Column(Boolean, default=True, comment="是否激活")
    created_at = Column(TIMESTAMP, default=func.current_timestamp())
    updated_at = Column(TIMESTAMP, default=func.current_timestamp(), onupdate=func.current_timestamp())
    created_by = Column(BigInteger, ForeignKey("users.id"), comment="创建者ID")
    
    # Relationships
    portfolios = relationship("Portfolio", back_populates="team")
    creator = relationship("User")
    members = relationship("TeamMember", back_populates="team", cascade="all, delete-orphan")


class TeamMember(Base):
    __tablename__ = "team_members"
    
    id = Column(BigInteger, primary_key=True, autoincrement=True)
    team_id = Column(BigInteger, ForeignKey("teams.id", ondelete="CASCADE"), nullable=False)
    user_id = Column(BigInteger, ForeignKey("users.id", ondelete="CASCADE"), nullable=False)
    joined_at = Column(TIMESTAMP, default=func.current_timestamp())
    
    # Relationships
    team = relationship("Team", back_populates="members")
    user = relationship("User")


# ==========================================
# 投资组合管理
# ==========================================

class Portfolio(Base):
    __tablename__ = "portfolios"
    
    id = Column(BigInteger, primary_key=True, autoincrement=True)
    
    # 1Token系统信息
    fund_name = Column(String(255), nullable=False, comment="1Token投组标识")
    fund_alias = Column(String(255), comment="1Token投组显示名称")
    inception_time = Column(TIMESTAMP, comment="1Token投组创始时间")
    
    # Binance子账号信息
    account_name = Column(String(255), comment="Binance子账号名称")
    account_alias = Column(String(255), comment="Binance子账号别名")
    
    # Ceffu钱包信息
    ceffu_wallet_id = Column(String(100), comment="Ceffu钱包数字ID")
    ceffu_wallet_name = Column(String(255), comment="Ceffu钱包显示名称")
    
    # 关联关系
    team_id = Column(BigInteger, ForeignKey("teams.id"), comment="所属交易团队ID")
    parent_id = Column(BigInteger, ForeignKey("portfolios.id"), comment="上级投资组合ID")
    
    # 状态和元数据
    is_active = Column(Boolean, default=True, comment="是否激活")
    created_at = Column(TIMESTAMP, default=func.current_timestamp())
    updated_at = Column(TIMESTAMP, default=func.current_timestamp(), onupdate=func.current_timestamp())
    created_by = Column(BigInteger, ForeignKey("users.id"), comment="创建者ID")
    
    # Relationships
    team = relationship("Team", back_populates="portfolios")
    parent = relationship("Portfolio", remote_side=[id])
    children = relationship("Portfolio")
    creator = relationship("User")


# ==========================================
# 数据快照系统
# ==========================================

class NavSnapshot(Base):
    __tablename__ = "nav_snapshots"
    
    id = Column(BigInteger, primary_key=True, autoincrement=True)
    snapshot_at = Column(TIMESTAMP, nullable=False, comment="快照时间(整点)")
    nav = Column(DECIMAL(20, 8), nullable=False, comment="NAV值")
    created_at = Column(TIMESTAMP, default=func.current_timestamp())


class RateSnapshot(Base):
    __tablename__ = "rate_snapshots"
    
    id = Column(BigInteger, primary_key=True, autoincrement=True)
    snapshot_at = Column(TIMESTAMP, nullable=False, comment="快照时间(整点)")
    base_currency = Column(String(10), nullable=False, default="USD", comment="基础货币")
    target_currency = Column(String(10), nullable=False, default="CNY", comment="目标货币")
    exchange_rate = Column(DECIMAL(20, 8), nullable=False, comment="汇率值")
    snapshot_date = Column(TIMESTAMP, nullable=False, comment="快照日期")
    created_at = Column(TIMESTAMP, default=func.current_timestamp())


class AssetsSnapshot(Base):
    __tablename__ = "assets_snapshots"
    
    id = Column(BigInteger, primary_key=True, autoincrement=True)
    snapshot_at = Column(TIMESTAMP, nullable=False, comment="快照时间(整点)")
    wallet_id = Column(String(100), comment="钱包ID")
    asset_symbol = Column(String(20), comment="资产符号")
    balance = Column(DECIMAL(20, 8), comment="资产余额")
    assets_value = Column(DECIMAL(20, 8), nullable=False, comment="资产USD价值")
    snapshot_date = Column(TIMESTAMP, nullable=False, comment="快照日期")
    created_at = Column(TIMESTAMP, default=func.current_timestamp())


# ==========================================
# 收益管理系统
# ==========================================

class AccProfitUserSnapshot(Base):
    __tablename__ = "acc_profit_user_snapshots"
    
    id = Column(BigInteger, primary_key=True, autoincrement=True)
    snapshot_at = Column(TIMESTAMP, nullable=False, comment="快照时间(整点)")
    acc_profit = Column(DECIMAL(20, 8), nullable=False, comment="累计收益USD")
    created_at = Column(TIMESTAMP, default=func.current_timestamp())


class AccProfitPlatformSnapshot(Base):
    __tablename__ = "acc_profit_platform_snapshots"
    
    id = Column(BigInteger, primary_key=True, autoincrement=True)
    snapshot_at = Column(TIMESTAMP, nullable=False, comment="快照时间(整点)")
    acc_profit = Column(DECIMAL(20, 8), nullable=False, comment="累计收益USD")
    created_at = Column(TIMESTAMP, default=func.current_timestamp())


class AccProfitTeamSnapshot(Base):
    __tablename__ = "acc_profit_team_snapshots"
    
    id = Column(BigInteger, primary_key=True, autoincrement=True)
    portfolio_id = Column(BigInteger, ForeignKey("portfolios.id"), nullable=False, comment="投资组合ID")
    snapshot_at = Column(TIMESTAMP, nullable=False, comment="快照时间(整点)")
    acc_profit = Column(DECIMAL(20, 8), nullable=False, comment="累计分给团队的收益USD")
    created_at = Column(TIMESTAMP, default=func.current_timestamp())
    
    # Relationships
    portfolio = relationship("Portfolio")


# API [19] - 投资组合每小时累计收益快照模型
class AccProfitFromPortfolio(Base):
    __tablename__ = "acc_profit_from_portfolio"
    
    id = Column(BigInteger, primary_key=True, autoincrement=True)
    portfolio_id = Column(BigInteger, ForeignKey("portfolios.id"), nullable=False, comment="投资组合在数据库中的id")
    snapshot_at = Column(TIMESTAMP, nullable=False, comment="整时快照秒级时间戳")
    acc_profit = Column(DECIMAL(20, 8), nullable=False, comment="投资组合累计到snapshotAt时刻的收益")
    created_at = Column(TIMESTAMP, default=func.current_timestamp(), comment="这条记录的入库时间")
    
    # Relationships
    portfolio = relationship("Portfolio")


class AccProfitPortfolioSnapshot(Base):
    __tablename__ = "acc_profit_portfolio_snapshots"
    
    id = Column(BigInteger, primary_key=True, autoincrement=True)
    portfolio_id = Column(BigInteger, ForeignKey("portfolios.id"), nullable=False, comment="投资组合ID")
    hour_index = Column(BigInteger, nullable=False, comment="小时索引")
    acc_points = Column(DECIMAL(20, 8), nullable=False, comment="累计收益点数")
    created_at = Column(TIMESTAMP, default=func.current_timestamp())
    
    # Relationships
    portfolio = relationship("Portfolio")


class ProfitAllocationRatio(Base):
    __tablename__ = "profit_allocation_ratios"
    
    id = Column(BigInteger, primary_key=True, autoincrement=True)
    portfolio_id = Column(BigInteger, ForeignKey("portfolios.id"), nullable=False, comment="投资组合ID")
    version = Column(Integer, nullable=False, default=1, comment="版本号")
    to_team = Column(Integer, nullable=False, comment="团队分配比例(10000=100%)")
    to_platform = Column(Integer, nullable=False, comment="平台分配比例(10000=100%)")
    to_user = Column(Integer, nullable=False, comment="用户分配比例(10000=100%)")
    created_at = Column(TIMESTAMP, default=func.current_timestamp())
    created_by = Column(BigInteger, ForeignKey("users.id"), comment="创建者ID")
    
    # Relationships
    portfolio = relationship("Portfolio")
    creator = relationship("User")


# API [27] - 收益分配记录/日志模型
class ProfitAllocationLog(Base):
    __tablename__ = "profit_allocation_logs"
    
    id = Column(BigInteger, primary_key=True, autoincrement=True)
    portfolio_id = Column(BigInteger, ForeignKey("portfolios.id"), nullable=False, comment="关联的投资组合id")
    hour_end_at = Column(TIMESTAMP, nullable=False, comment="整时结算时刻时间戳")
    
    # 快照关联
    hourly_snapshot_prev_id = Column(BigInteger, ForeignKey("acc_profit_from_portfolio.id"), comment="acc_profit_from_portfolio前一个小时id")
    hourly_snapshot_curr_id = Column(BigInteger, ForeignKey("acc_profit_from_portfolio.id"), comment="acc_profit_from_portfolio后一个小时id") 
    hourly_profit = Column(DECIMAL(20, 8), nullable=False, comment="一小时收益，可正数/负数")
    
    # 分配给三方的收益
    profit_to_team = Column(DECIMAL(20, 8), nullable=False, comment="分配给团队的收益，可正数/负数")
    profit_to_user = Column(DECIMAL(20, 8), nullable=False, comment="分配给用户的收益，可正数/负数")
    profit_to_platform = Column(DECIMAL(20, 8), nullable=False, comment="分配给平台的收益，可正数/负数")
    
    # 分配比例依据
    allocation_ratio_id = Column(BigInteger, ForeignKey("profit_allocation_ratios.id"), nullable=False, comment="分配比例参数的id")
    
    created_at = Column(TIMESTAMP, default=func.current_timestamp(), comment="本条记录入库时间")
    
    # Relationships  
    portfolio = relationship("Portfolio")
    allocation_ratio = relationship("ProfitAllocationRatio")
    hourly_snapshot_prev = relationship("AccProfitFromPortfolio", foreign_keys=[hourly_snapshot_prev_id])
    hourly_snapshot_curr = relationship("AccProfitFromPortfolio", foreign_keys=[hourly_snapshot_curr_id])


# API [28] - 用户一小时内收益变动模型
class HourlyProfitUser(Base):
    __tablename__ = "hourly_profit_user"
    
    id = Column(BigInteger, primary_key=True, autoincrement=True)
    hour_end_at = Column(TIMESTAMP, nullable=False, comment="小时结束时间戳")
    profit_delta = Column(DECIMAL(20, 8), nullable=False, comment="当前小时变动")
    delta_from_fund = Column(DECIMAL(20, 8), nullable=False, comment="基金量化收益，可正可负")
    delta_from_reallocation = Column(DECIMAL(20, 8), nullable=False, comment="调账，转出为负转入为正")
    created_at = Column(TIMESTAMP, default=func.current_timestamp())


# API [29] - 平台一小时内收益变动模型  
class HourlyProfitPlatform(Base):
    __tablename__ = "hourly_profit_platform"
    
    id = Column(BigInteger, primary_key=True, autoincrement=True)
    hour_end_at = Column(TIMESTAMP, nullable=False, comment="小时结束时间戳")
    profit_delta = Column(DECIMAL(20, 8), nullable=False, comment="一小时内profit变动，可正数或负数")
    delta_from_fund = Column(DECIMAL(20, 8), nullable=False, comment="基金量化收益")
    delta_from_reallocation = Column(DECIMAL(20, 8), nullable=False, comment="调账变动")
    delta_from_withdraw = Column(DECIMAL(20, 8), nullable=False, comment="提现变动")
    created_at = Column(TIMESTAMP, default=func.current_timestamp())


# API [30] - 团队投资组合一小时内收益变动模型
class HourlyProfitTeam(Base):
    __tablename__ = "hourly_profit_team"
    
    id = Column(BigInteger, primary_key=True, autoincrement=True)
    portfolio_id = Column(BigInteger, ForeignKey("portfolios.id"), nullable=False, comment="投资组合ID")
    hour_end_at = Column(TIMESTAMP, nullable=False, comment="小时结束时间戳")
    profit_delta = Column(DECIMAL(20, 8), nullable=False, comment="一小时内profit变动，可正可负")
    delta_from_fund = Column(DECIMAL(20, 8), nullable=False, comment="量化基金收益，盈利为正，回撤为负")
    delta_from_reallocation = Column(DECIMAL(20, 8), nullable=False, comment="调账进出，转出为负，转入为正")
    delta_from_withdraw = Column(DECIMAL(20, 8), nullable=False, comment="提现转出，应该是负数")
    created_at = Column(TIMESTAMP, default=func.current_timestamp())
    
    # Relationships
    portfolio = relationship("Portfolio")


class WithdrawalFromType(enum.Enum):
    team = "team"
    platform = "platform"


class AssetsType(enum.Enum):
    USDT = "USDT"
    USDC = "USDC"


class ProfitWithdrawal(Base):
    __tablename__ = "profit_withdrawals"
    
    id = Column(BigInteger, primary_key=True, autoincrement=True)
    from_type = Column(Enum(WithdrawalFromType), nullable=False, comment="提取类型")
    team_id = Column(BigInteger, ForeignKey("teams.id"), comment="团队ID(team类型时必填)")
    chain_id = Column(String(100), nullable=False, comment="链ID")
    transaction_hash = Column(String(255), nullable=False, unique=True, comment="交易哈希")
    transaction_time = Column(TIMESTAMP, nullable=False, comment="交易时间")
    usd_value = Column(DECIMAL(20, 8), nullable=False, comment="USD价值")
    assets = Column(Enum(AssetsType), nullable=False, comment="资产类型")
    assets_amount = Column(DECIMAL(20, 8), nullable=False, comment="资产数量")
    created_at = Column(TIMESTAMP, default=func.current_timestamp())
    created_by = Column(BigInteger, ForeignKey("users.id"), comment="创建者ID")
    
    # Relationships
    team = relationship("Team")
    creator = relationship("User")


class ReallocationFromType(enum.Enum):
    platform = "platform"
    user = "user"
    team = "team"


class ReallocationToType(enum.Enum):
    user = "user"
    platform = "platform"
    team = "team"


class ProfitReallocation(Base):
    __tablename__ = "profit_reallocations"
    
    id = Column(BigInteger, primary_key=True, autoincrement=True)
    from_type = Column(Enum(ReallocationFromType), nullable=False, comment="转出类型")
    to_type = Column(Enum(ReallocationToType), nullable=False, comment="转入类型")
    from_team_id = Column(BigInteger, ForeignKey("teams.id"), comment="转出团队ID")
    to_team_id = Column(BigInteger, ForeignKey("teams.id"), comment="转入团队ID")
    usd_value = Column(DECIMAL(20, 8), nullable=False, comment="USD价值")
    reason = Column(Text, nullable=False, comment="调账原因")
    created_at = Column(TIMESTAMP, default=func.current_timestamp())
    created_by = Column(BigInteger, ForeignKey("users.id"), comment="创建者ID")
    
    # Relationships
    from_team = relationship("Team", foreign_keys=[from_team_id])
    to_team = relationship("Team", foreign_keys=[to_team_id])
    creator = relationship("User")


# ==========================================
# 黑名单管理
# ==========================================

class BlacklistAddress(Base):
    __tablename__ = "blacklist_addresses"
    
    id = Column(BigInteger, primary_key=True, autoincrement=True)
    address = Column(String(255), nullable=False, unique=True, comment="地址(小写)")
    created_at = Column(TIMESTAMP, default=func.current_timestamp())
    created_by = Column(BigInteger, ForeignKey("users.id"), comment="创建者ID")
    
    # Relationships
    creator = relationship("User")


# ==========================================
# 系统操作日志
# ==========================================

class OperationLog(Base):
    __tablename__ = "operation_logs"
    
    id = Column(BigInteger, primary_key=True, autoincrement=True)
    user_id = Column(BigInteger, ForeignKey("users.id"), comment="操作用户ID")
    operation = Column(String(100), nullable=False, comment="操作类型")
    resource_type = Column(String(50), comment="资源类型")
    resource_id = Column(String(100), comment="资源ID")
    details = Column(JSON, comment="操作详情")
    ip_address = Column(String(45), comment="IP地址")
    user_agent = Column(Text, comment="用户代理")
    created_at = Column(TIMESTAMP, default=func.current_timestamp())
    
    # Relationships
    user = relationship("User")