-- OneToken Fund Management System Database Schema
-- 基于Server端Restful API需求设计

-- ==========================================
-- 用户认证和权限管理
-- ==========================================

-- 用户表
CREATE TABLE users (
    id BIGINT PRIMARY KEY AUTO_INCREMENT,
    email VARCHAR(255) NOT NULL UNIQUE COMMENT '用户邮箱',
    password_hash VARCHAR(255) NOT NULL COMMENT '密码哈希',
    is_super BOOLEAN DEFAULT FALSE COMMENT '是否超级管理员',
    is_active BOOLEAN DEFAULT TRUE COMMENT '是否激活',
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    INDEX idx_email (email),
    INDEX idx_is_super (is_super)
) COMMENT='用户基础信息表';

-- 权限定义表
CREATE TABLE permissions (
    id VARCHAR(50) PRIMARY KEY COMMENT '权限标识符',
    label VARCHAR(100) NOT NULL COMMENT '权限显示名称',
    description TEXT COMMENT '权限描述',
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
) COMMENT='系统权限定义表';

-- 用户权限关联表
CREATE TABLE user_permissions (
    id BIGINT PRIMARY KEY AUTO_INCREMENT,
    user_id BIGINT NOT NULL,
    permission_id VARCHAR(50) NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    FOREIGN KEY (permission_id) REFERENCES permissions(id) ON DELETE CASCADE,
    UNIQUE KEY uk_user_permission (user_id, permission_id)
) COMMENT='用户权限关联表';

-- 用户登录会话表
CREATE TABLE user_sessions (
    id BIGINT PRIMARY KEY AUTO_INCREMENT,
    user_id BIGINT NOT NULL,
    token VARCHAR(500) NOT NULL UNIQUE COMMENT 'JWT Token',
    expires_at TIMESTAMP NOT NULL COMMENT '过期时间',
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    INDEX idx_token (token),
    INDEX idx_expires_at (expires_at)
) COMMENT='用户登录会话表';

-- ==========================================
-- 交易团队管理
-- ==========================================

-- 交易团队表
CREATE TABLE teams (
    id BIGINT PRIMARY KEY AUTO_INCREMENT,
    name VARCHAR(255) NOT NULL UNIQUE COMMENT '团队名称',
    description TEXT COMMENT '团队描述',
    is_active BOOLEAN DEFAULT TRUE COMMENT '是否激活',
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    created_by BIGINT COMMENT '创建者ID',
    FOREIGN KEY (created_by) REFERENCES users(id),
    INDEX idx_name (name)
) COMMENT='交易团队表';

-- ==========================================
-- 投资组合管理
-- ==========================================

-- 投资组合表
CREATE TABLE portfolios (
    id BIGINT PRIMARY KEY AUTO_INCREMENT,
    
    -- 1Token系统信息
    fund_name VARCHAR(255) NOT NULL COMMENT '1Token投组标识',
    fund_alias VARCHAR(255) COMMENT '1Token投组显示名称',
    inception_time TIMESTAMP COMMENT '1Token投组创始时间',
    
    -- Binance子账号信息
    account_name VARCHAR(255) COMMENT 'Binance子账号名称',
    account_alias VARCHAR(255) COMMENT 'Binance子账号别名',
    
    -- Ceffu钱包信息
    ceffu_wallet_id VARCHAR(100) COMMENT 'Ceffu钱包数字ID',
    ceffu_wallet_name VARCHAR(255) COMMENT 'Ceffu钱包显示名称',
    
    -- 关联关系
    team_id BIGINT COMMENT '所属交易团队ID',
    parent_id BIGINT COMMENT '上级投资组合ID',
    
    -- 状态和元数据
    is_active BOOLEAN DEFAULT TRUE COMMENT '是否激活',
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    created_by BIGINT COMMENT '创建者ID',
    
    FOREIGN KEY (team_id) REFERENCES teams(id),
    FOREIGN KEY (parent_id) REFERENCES portfolios(id),
    FOREIGN KEY (created_by) REFERENCES users(id),
    
    INDEX idx_fund_name (fund_name),
    INDEX idx_ceffu_wallet_id (ceffu_wallet_id),
    INDEX idx_team_id (team_id)
) COMMENT='投资组合表';

-- ==========================================
-- 数据快照系统
-- ==========================================

-- NAV快照表
CREATE TABLE nav_snapshots (
    id BIGINT PRIMARY KEY AUTO_INCREMENT,
    snapshot_at TIMESTAMP NOT NULL COMMENT '快照时间(整点)',
    nav DECIMAL(20,8) NOT NULL COMMENT 'NAV值',
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_snapshot_at (snapshot_at)
) COMMENT='NAV每小时快照表';

-- 汇率快照表
CREATE TABLE rate_snapshots (
    id BIGINT PRIMARY KEY AUTO_INCREMENT,
    snapshot_at TIMESTAMP NOT NULL COMMENT '快照时间(整点)',
    exchange_rate DECIMAL(20,8) NOT NULL COMMENT '汇率值',
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_snapshot_at (snapshot_at)
) COMMENT='汇率每小时快照表';

-- 资产快照表
CREATE TABLE assets_snapshots (
    id BIGINT PRIMARY KEY AUTO_INCREMENT,
    snapshot_at TIMESTAMP NOT NULL COMMENT '快照时间(整点)',
    assets_value DECIMAL(20,8) NOT NULL COMMENT '资产USD价值',
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_snapshot_at (snapshot_at)
) COMMENT='净资产每小时快照表';

-- ==========================================
-- 收益管理系统
-- ==========================================

-- 用户累计收益快照表
CREATE TABLE acc_profit_user_snapshots (
    id BIGINT PRIMARY KEY AUTO_INCREMENT,
    snapshot_at TIMESTAMP NOT NULL COMMENT '快照时间(整点)',
    acc_profit DECIMAL(20,8) NOT NULL COMMENT '累计收益USD',
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_snapshot_at (snapshot_at)
) COMMENT='用户累计收益每小时快照表';

-- 平台累计收益快照表
CREATE TABLE acc_profit_platform_snapshots (
    id BIGINT PRIMARY KEY AUTO_INCREMENT,
    snapshot_at TIMESTAMP NOT NULL COMMENT '快照时间(整点)',
    acc_profit DECIMAL(20,8) NOT NULL COMMENT '累计收益USD',
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_snapshot_at (snapshot_at)
) COMMENT='平台累计收益每小时快照表';

-- 团队累计收益快照表
CREATE TABLE acc_profit_team_snapshots (
    id BIGINT PRIMARY KEY AUTO_INCREMENT,
    portfolio_id BIGINT NOT NULL COMMENT '投资组合ID',
    snapshot_at TIMESTAMP NOT NULL COMMENT '快照时间(整点)',
    acc_profit DECIMAL(20,8) NOT NULL COMMENT '累计分给团队的收益USD',
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (portfolio_id) REFERENCES portfolios(id),
    INDEX idx_portfolio_snapshot (portfolio_id, snapshot_at),
    INDEX idx_snapshot_at (snapshot_at)
) COMMENT='团队累计收益每小时快照表';

-- 投资组合原始收益快照表
CREATE TABLE acc_profit_portfolio_snapshots (
    id BIGINT PRIMARY KEY AUTO_INCREMENT,
    portfolio_id BIGINT NOT NULL COMMENT '投资组合ID',
    hour_index BIGINT NOT NULL COMMENT '小时索引',
    acc_points DECIMAL(20,8) NOT NULL COMMENT '累计收益点数',
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (portfolio_id) REFERENCES portfolios(id),
    INDEX idx_portfolio_hour (portfolio_id, hour_index)
) COMMENT='投资组合原始累计收益快照表';

-- 收益分配比例配置表
CREATE TABLE profit_allocation_ratios (
    id BIGINT PRIMARY KEY AUTO_INCREMENT,
    portfolio_id BIGINT NOT NULL COMMENT '投资组合ID',
    version INT NOT NULL DEFAULT 1 COMMENT '版本号',
    to_team INT NOT NULL COMMENT '团队分配比例(10000=100%)',
    to_platform INT NOT NULL COMMENT '平台分配比例(10000=100%)',
    to_user INT NOT NULL COMMENT '用户分配比例(10000=100%)',
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    created_by BIGINT COMMENT '创建者ID',
    
    FOREIGN KEY (portfolio_id) REFERENCES portfolios(id),
    FOREIGN KEY (created_by) REFERENCES users(id),
    
    INDEX idx_portfolio_version (portfolio_id, version DESC),
    
    CONSTRAINT chk_allocation_sum CHECK (to_team + to_platform + to_user = 10000)
) COMMENT='收益分配比例配置表';

-- 收益提取记录表
CREATE TABLE profit_withdrawals (
    id BIGINT PRIMARY KEY AUTO_INCREMENT,
    from_type ENUM('team', 'platform') NOT NULL COMMENT '提取类型',
    team_id BIGINT COMMENT '团队ID(team类型时必填)',
    chain_id VARCHAR(100) NOT NULL COMMENT '链ID',
    transaction_hash VARCHAR(255) NOT NULL UNIQUE COMMENT '交易哈希',
    transaction_time TIMESTAMP NOT NULL COMMENT '交易时间',
    usd_value DECIMAL(20,8) NOT NULL COMMENT 'USD价值',
    assets ENUM('USDT', 'USDC') NOT NULL COMMENT '资产类型',
    assets_amount DECIMAL(20,8) NOT NULL COMMENT '资产数量',
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    created_by BIGINT COMMENT '创建者ID',
    
    FOREIGN KEY (team_id) REFERENCES teams(id),
    FOREIGN KEY (created_by) REFERENCES users(id),
    
    INDEX idx_from_type_time (from_type, transaction_time DESC),
    INDEX idx_team_time (team_id, transaction_time DESC),
    INDEX idx_transaction_hash (transaction_hash)
) COMMENT='收益提取记录表';

-- 收益调账记录表
CREATE TABLE profit_reallocations (
    id BIGINT PRIMARY KEY AUTO_INCREMENT,
    from_type ENUM('platform', 'user', 'team') NOT NULL COMMENT '转出类型',
    to_type ENUM('user', 'platform', 'team') NOT NULL COMMENT '转入类型',
    from_team_id BIGINT COMMENT '转出团队ID',
    to_team_id BIGINT COMMENT '转入团队ID',
    usd_value DECIMAL(20,8) NOT NULL COMMENT 'USD价值',
    reason TEXT NOT NULL COMMENT '调账原因',
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    created_by BIGINT COMMENT '创建者ID',
    
    FOREIGN KEY (from_team_id) REFERENCES teams(id),
    FOREIGN KEY (to_team_id) REFERENCES teams(id),
    FOREIGN KEY (created_by) REFERENCES users(id),
    
    INDEX idx_from_to_time (from_type, to_type, created_at DESC),
    INDEX idx_created_at (created_at DESC)
) COMMENT='收益调账记录表';

-- ==========================================
-- 黑名单管理
-- ==========================================

-- 黑名单地址表
CREATE TABLE blacklist_addresses (
    id BIGINT PRIMARY KEY AUTO_INCREMENT,
    address VARCHAR(255) NOT NULL UNIQUE COMMENT '地址(小写)',
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    created_by BIGINT COMMENT '创建者ID',
    
    FOREIGN KEY (created_by) REFERENCES users(id),
    INDEX idx_address (address)
) COMMENT='黑名单地址表';

-- ==========================================
-- 系统操作日志
-- ==========================================

-- 操作日志表
CREATE TABLE operation_logs (
    id BIGINT PRIMARY KEY AUTO_INCREMENT,
    user_id BIGINT COMMENT '操作用户ID',
    operation VARCHAR(100) NOT NULL COMMENT '操作类型',
    resource_type VARCHAR(50) COMMENT '资源类型',
    resource_id VARCHAR(100) COMMENT '资源ID',
    details JSON COMMENT '操作详情',
    ip_address VARCHAR(45) COMMENT 'IP地址',
    user_agent TEXT COMMENT '用户代理',
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    
    FOREIGN KEY (user_id) REFERENCES users(id),
    INDEX idx_user_time (user_id, created_at DESC),
    INDEX idx_operation_time (operation, created_at DESC)
) COMMENT='系统操作日志表';

-- ==========================================
-- 初始化数据
-- ==========================================

-- 插入系统权限
INSERT INTO permissions (id, label, description) VALUES
('user', '用户管理', '创建、编辑、删除用户，管理用户权限'),
('team', '团队管理', '创建、编辑、删除交易团队'),
('profit', '收益管理', '管理收益分配比例、提取记录、调账操作'),
('portfolio', '投资组合管理', '管理投资组合与团队绑定关系'),
('blacklist', '黑名单管理', '管理黑名单地址');

-- 创建超级管理员用户 (密码: admin123)
INSERT INTO users (email, password_hash, is_super) VALUES
('admin@onetoken.com', '$2b$12$LQv3c1yqBWVHxkd0LHAkCOYz6TtxMQJqhN8/LewVyKyqK9Z8kjLWC', TRUE);

-- 为超级管理员分配所有权限
INSERT INTO user_permissions (user_id, permission_id)
SELECT 1, id FROM permissions;

-- 创建默认团队
INSERT INTO teams (name, description, created_by) VALUES
('默认团队', '系统默认交易团队', 1);

-- ==========================================
-- 视图和存储过程
-- ==========================================

-- 创建用户详细信息视图
CREATE VIEW user_details AS
SELECT 
    u.id,
    u.email,
    u.is_super,
    u.is_active,
    u.created_at,
    u.updated_at,
    JSON_ARRAYAGG(p.id) as permissions
FROM users u
LEFT JOIN user_permissions up ON u.id = up.user_id
LEFT JOIN permissions p ON up.permission_id = p.id
WHERE u.is_active = TRUE
GROUP BY u.id, u.email, u.is_super, u.is_active, u.created_at, u.updated_at;

-- 创建投资组合详细信息视图
CREATE VIEW portfolio_details AS
SELECT 
    p.*,
    t.name as team_name,
    parent.fund_name as parent_fund_name
FROM portfolios p
LEFT JOIN teams t ON p.team_id = t.id
LEFT JOIN portfolios parent ON p.parent_id = parent.id
WHERE p.is_active = TRUE;