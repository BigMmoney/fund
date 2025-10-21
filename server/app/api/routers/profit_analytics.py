"""
收益分析路由 - API [19, 27-33] 
投资组合累计收益、每小时收益变动、累计收益快照等
"""
from typing import Dict, Any, List, Optional
from fastapi import APIRouter, Depends, Query
from sqlalchemy.orm import Session
from sqlalchemy import desc, and_
from datetime import datetime

from server.app.database import get_db
from server.app.models import (
    AccProfitFromPortfolio, ProfitAllocationLog, HourlyProfitUser, 
    HourlyProfitPlatform, HourlyProfitTeam, AccProfitUserSnapshot,
    AccProfitPlatformSnapshot, AccProfitTeamSnapshot, Portfolio
)
from server.app.auth import get_current_user
from server.app.responses import StandardResponse

router = APIRouter(tags=["Profit Analytics"])


# API [19] GET /acc_profit_from_portfolio  
@router.get("/acc_profit_from_portfolio")
async def get_acc_profit_from_portfolio(
    limit: int = Query(100, description="限制返回的元素数量"),
    offset: int = Query(0, description="列表起始元素下标"),
    portfolio_id: Optional[int] = Query(None, description="投资组合id"),
    snapshot_at: Optional[List[int]] = Query(None, description="快照时间(可多值)"),
    snapshot_at_gte: Optional[int] = Query(None, description="快照时间 >= 指定时间"),
    snapshot_at_lt: Optional[int] = Query(None, description="快照时间 < 指定时间"),
    current_user = Depends(get_current_user),
    db: Session = Depends(get_db)
) -> Dict[str, Any]:
    """投资组合每小时累计收益 - 1token 投资组合的原始累计收益快照"""
    try:
        # 构建基础查询
        query = db.query(AccProfitFromPortfolio)
        
        # 应用过滤条件
        if portfolio_id:
            query = query.filter(AccProfitFromPortfolio.portfolio_id == portfolio_id)
        
        if snapshot_at:
            # 处理多值时间过滤
            timestamp_filters = []
            for ts in snapshot_at:
                timestamp_filters.append(AccProfitFromPortfolio.snapshot_at == datetime.fromtimestamp(ts))
            query = query.filter(or_(*timestamp_filters))
        
        if snapshot_at_gte:
            query = query.filter(AccProfitFromPortfolio.snapshot_at >= datetime.fromtimestamp(snapshot_at_gte))
        
        if snapshot_at_lt:
            query = query.filter(AccProfitFromPortfolio.snapshot_at < datetime.fromtimestamp(snapshot_at_lt))
        
        # 获取总数
        total = query.count()
        
        # 应用排序（按snapshotAt倒序）
        query = query.order_by(desc(AccProfitFromPortfolio.snapshot_at))
        
        # 应用分页
        query = query.offset(offset).limit(limit)
        
        # 执行查询
        snapshots = query.all()
        
        # 格式化数据
        snapshot_list = []
        for snapshot in snapshots:
            snapshot_list.append({
                "id": snapshot.id,
                "portfolioId": snapshot.portfolio_id,
                "snapshotAt": int(snapshot.snapshot_at.timestamp()),
                "accProfit": str(snapshot.acc_profit),
                "createdAt": int(snapshot.created_at.timestamp())
            })
        
        return StandardResponse.list_success(snapshot_list, total)
        
    except Exception as e:
        return StandardResponse.error(f"Failed to get portfolio profit snapshots: {str(e)}")


# API [27] GET /profit_allocation_logs
@router.get("/profit_allocation_logs")
async def get_profit_allocation_logs(
    limit: int = Query(100, description="限制返回的元素数量"),
    offset: int = Query(0, description="列表起始元素下标"),
    portfolio_id: Optional[int] = Query(None, description="投资组合id"),
    hour_end_at: Optional[List[int]] = Query(None, description="结算时间(可多值)"),
    hour_end_at_gte: Optional[int] = Query(None, description="结算时间 >= 指定时间"),
    hour_end_at_lt: Optional[int] = Query(None, description="结算时间 < 指定时间"),
    current_user = Depends(get_current_user),
    db: Session = Depends(get_db)
) -> Dict[str, Any]:
    """收益分配记录 - 投资组合每小时收益分配日志"""
    try:
        # 构建基础查询
        query = db.query(ProfitAllocationLog)
        
        # 应用过滤条件
        if portfolio_id:
            query = query.filter(ProfitAllocationLog.portfolio_id == portfolio_id)
        
        if hour_end_at:
            # 处理多值时间过滤
            from sqlalchemy import or_
            timestamp_filters = []
            for ts in hour_end_at:
                timestamp_filters.append(ProfitAllocationLog.hour_end_at == datetime.fromtimestamp(ts))
            query = query.filter(or_(*timestamp_filters))
        
        if hour_end_at_gte:
            query = query.filter(ProfitAllocationLog.hour_end_at >= datetime.fromtimestamp(hour_end_at_gte))
        
        if hour_end_at_lt:
            query = query.filter(ProfitAllocationLog.hour_end_at < datetime.fromtimestamp(hour_end_at_lt))
        
        # 获取总数
        total = query.count()
        
        # 应用排序（按hourEndAt倒序）
        query = query.order_by(desc(ProfitAllocationLog.hour_end_at))
        
        # 应用分页
        query = query.offset(offset).limit(limit)
        
        # 执行查询
        logs = query.all()
        
        # 格式化数据
        log_list = []
        for log in logs:
            log_list.append({
                "id": log.id,
                "portfolioId": log.portfolio_id,
                "hourEndAt": int(log.hour_end_at.timestamp()),
                "hourlySnapshotPrev": log.hourly_snapshot_prev_id,
                "hourlySnapshotCurr": log.hourly_snapshot_curr_id,
                "hourlyProfit": str(log.hourly_profit),
                "profitToTeam": str(log.profit_to_team),
                "profitToUser": str(log.profit_to_user),
                "profitToPlatform": str(log.profit_to_platform),
                "allocationRatioId": log.allocation_ratio_id,
                "createdAt": int(log.created_at.timestamp())
            })
        
        return StandardResponse.list_success(log_list, total)
        
    except Exception as e:
        return StandardResponse.error(f"Failed to get profit allocation logs: {str(e)}")


# API [28] GET /hourly_profit_user
@router.get("/hourly_profit_user")
async def get_hourly_profit_user(
    limit: int = Query(100, description="限制返回的元素数量"),
    offset: int = Query(0, description="列表起始元素下标"),
    hour_end_at: Optional[List[int]] = Query(None, description="小时结束时间(可多值)"),
    hour_end_at_gte: Optional[int] = Query(None, description="时间 >= 指定时间"),
    hour_end_at_lt: Optional[int] = Query(None, description="时间 < 指定时间"),
    current_user = Depends(get_current_user),
    db: Session = Depends(get_db)
) -> Dict[str, Any]:
    """用户一小时内收益变动"""
    try:
        # 构建基础查询
        query = db.query(HourlyProfitUser)
        
        # 应用时间过滤
        if hour_end_at:
            from sqlalchemy import or_
            timestamp_filters = []
            for ts in hour_end_at:
                timestamp_filters.append(HourlyProfitUser.hour_end_at == datetime.fromtimestamp(ts))
            query = query.filter(or_(*timestamp_filters))
        
        if hour_end_at_gte:
            query = query.filter(HourlyProfitUser.hour_end_at >= datetime.fromtimestamp(hour_end_at_gte))
        
        if hour_end_at_lt:
            query = query.filter(HourlyProfitUser.hour_end_at < datetime.fromtimestamp(hour_end_at_lt))
        
        # 获取总数
        total = query.count()
        
        # 应用排序（按时间倒序）
        query = query.order_by(desc(HourlyProfitUser.hour_end_at))
        
        # 应用分页
        query = query.offset(offset).limit(limit)
        
        # 执行查询
        profits = query.all()
        
        # 格式化数据
        profit_list = []
        for profit in profits:
            profit_list.append({
                "id": profit.id,
                "hourEndAt": int(profit.hour_end_at.timestamp()),
                "profitDelta": str(profit.profit_delta),
                "deltaFromFund": str(profit.delta_from_fund),
                "deltaFromReallocation": str(profit.delta_from_reallocation),
                "createdAt": int(profit.created_at.timestamp())
            })
        
        return StandardResponse.list_success(profit_list, total)
        
    except Exception as e:
        return StandardResponse.error(f"Failed to get hourly user profit: {str(e)}")


# API [29] GET /hourly_profit_platform
@router.get("/hourly_profit_platform")
async def get_hourly_profit_platform(
    limit: int = Query(100, description="限制返回的元素数量"),
    offset: int = Query(0, description="列表起始元素下标"),
    hour_end_at: Optional[List[int]] = Query(None, description="小时结束时间(可多值)"),
    hour_end_at_gte: Optional[int] = Query(None, description="时间 >= 指定时间"),
    hour_end_at_lt: Optional[int] = Query(None, description="时间 < 指定时间"),
    current_user = Depends(get_current_user),
    db: Session = Depends(get_db)
) -> Dict[str, Any]:
    """平台一小时内收益变动"""
    try:
        # 构建基础查询
        query = db.query(HourlyProfitPlatform)
        
        # 应用时间过滤
        if hour_end_at:
            from sqlalchemy import or_
            timestamp_filters = []
            for ts in hour_end_at:
                timestamp_filters.append(HourlyProfitPlatform.hour_end_at == datetime.fromtimestamp(ts))
            query = query.filter(or_(*timestamp_filters))
        
        if hour_end_at_gte:
            query = query.filter(HourlyProfitPlatform.hour_end_at >= datetime.fromtimestamp(hour_end_at_gte))
        
        if hour_end_at_lt:
            query = query.filter(HourlyProfitPlatform.hour_end_at < datetime.fromtimestamp(hour_end_at_lt))
        
        # 获取总数
        total = query.count()
        
        # 应用排序（按时间倒序）
        query = query.order_by(desc(HourlyProfitPlatform.hour_end_at))
        
        # 应用分页
        query = query.offset(offset).limit(limit)
        
        # 执行查询
        profits = query.all()
        
        # 格式化数据
        profit_list = []
        for profit in profits:
            profit_list.append({
                "id": profit.id,
                "hourEndAt": int(profit.hour_end_at.timestamp()),
                "profitDelta": str(profit.profit_delta),
                "deltaFromFund": str(profit.delta_from_fund),
                "deltaFromReallocation": str(profit.delta_from_reallocation),
                "deltaFromWithdraw": str(profit.delta_from_withdraw),
                "createdAt": int(profit.created_at.timestamp())
            })
        
        return StandardResponse.list_success(profit_list, total)
        
    except Exception as e:
        return StandardResponse.error(f"Failed to get hourly platform profit: {str(e)}")


# API [30] GET /hourly_profit_team
@router.get("/hourly_profit_team")
async def get_hourly_profit_team(
    limit: int = Query(100, description="限制返回的元素数量"),
    offset: int = Query(0, description="列表起始元素下标"),
    portfolio_id: Optional[int] = Query(None, description="投资组合id"),
    hour_end_at: Optional[List[int]] = Query(None, description="小时结束时间(可多值)"),
    hour_end_at_gte: Optional[int] = Query(None, description="时间 >= 指定时间"),
    hour_end_at_lt: Optional[int] = Query(None, description="时间 < 指定时间"),
    current_user = Depends(get_current_user),
    db: Session = Depends(get_db)
) -> Dict[str, Any]:
    """团队的投资组合一小时内收益变动"""
    try:
        # 构建基础查询
        query = db.query(HourlyProfitTeam)
        
        # 应用过滤条件
        if portfolio_id:
            query = query.filter(HourlyProfitTeam.portfolio_id == portfolio_id)
        
        # 应用时间过滤
        if hour_end_at:
            from sqlalchemy import or_
            timestamp_filters = []
            for ts in hour_end_at:
                timestamp_filters.append(HourlyProfitTeam.hour_end_at == datetime.fromtimestamp(ts))
            query = query.filter(or_(*timestamp_filters))
        
        if hour_end_at_gte:
            query = query.filter(HourlyProfitTeam.hour_end_at >= datetime.fromtimestamp(hour_end_at_gte))
        
        if hour_end_at_lt:
            query = query.filter(HourlyProfitTeam.hour_end_at < datetime.fromtimestamp(hour_end_at_lt))
        
        # 获取总数
        total = query.count()
        
        # 应用排序（按时间倒序）
        query = query.order_by(desc(HourlyProfitTeam.hour_end_at))
        
        # 应用分页
        query = query.offset(offset).limit(limit)
        
        # 执行查询
        profits = query.all()
        
        # 格式化数据
        profit_list = []
        for profit in profits:
            profit_list.append({
                "id": profit.id,
                "portfolioId": profit.portfolio_id,
                "hourEndAt": int(profit.hour_end_at.timestamp()),
                "profitDelta": str(profit.profit_delta),
                "deltaFromFund": str(profit.delta_from_fund),
                "deltaFromReallocation": str(profit.delta_from_reallocation),
                "deltaFromWithdraw": str(profit.delta_from_withdraw),
                "createdAt": int(profit.created_at.timestamp())
            })
        
        return StandardResponse.list_success(profit_list, total)
        
    except Exception as e:
        return StandardResponse.error(f"Failed to get hourly team profit: {str(e)}")


# API [31] GET /acc_profit_user
@router.get("/acc_profit_user")
async def get_acc_profit_user(
    limit: int = Query(100, description="分页，每页数量"),
    offset: int = Query(0, description="当前页面，开始的下标"),
    snapshot_at: Optional[List[int]] = Query(None, description="快照时间(可多值)"),
    snapshot_at_gte: Optional[int] = Query(None, description="快照时间 >= 指定时间"),
    snapshot_at_lt: Optional[int] = Query(None, description="快照时间 < 指定时间"),
    current_user = Depends(get_current_user),
    db: Session = Depends(get_db)
) -> Dict[str, Any]:
    """用户收益累计(余额)，即用户虚拟账户余额，的每小时快照"""
    try:
        # 构建基础查询
        query = db.query(AccProfitUserSnapshot)
        
        # 应用时间过滤
        if snapshot_at:
            from sqlalchemy import or_
            timestamp_filters = []
            for ts in snapshot_at:
                timestamp_filters.append(AccProfitUserSnapshot.snapshot_at == datetime.fromtimestamp(ts))
            query = query.filter(or_(*timestamp_filters))
        
        if snapshot_at_gte:
            query = query.filter(AccProfitUserSnapshot.snapshot_at >= datetime.fromtimestamp(snapshot_at_gte))
        
        if snapshot_at_lt:
            query = query.filter(AccProfitUserSnapshot.snapshot_at < datetime.fromtimestamp(snapshot_at_lt))
        
        # 获取总数
        total = query.count()
        
        # 应用排序（按快照时间倒序）
        query = query.order_by(desc(AccProfitUserSnapshot.snapshot_at))
        
        # 应用分页
        query = query.offset(offset).limit(limit)
        
        # 执行查询
        snapshots = query.all()
        
        # 格式化数据
        snapshot_list = []
        for snapshot in snapshots:
            snapshot_list.append({
                "id": snapshot.id,
                "snapshotAt": int(snapshot.snapshot_at.timestamp()),
                "accProfit": str(snapshot.acc_profit),
                "createdAt": int(snapshot.created_at.timestamp())
            })
        
        return StandardResponse.list_success(snapshot_list, total)
        
    except Exception as e:
        return StandardResponse.error(f"Failed to get user profit snapshots: {str(e)}")


# API [32] GET /acc_profit_platform
@router.get("/acc_profit_platform")
async def get_acc_profit_platform(
    limit: int = Query(100, description="分页，每页数量"),
    offset: int = Query(0, description="当前页面，开始的下标"),
    snapshot_at: Optional[List[int]] = Query(None, description="快照时间(可多值)"),
    snapshot_at_gte: Optional[int] = Query(None, description="快照时间 >= 指定时间"),
    snapshot_at_lt: Optional[int] = Query(None, description="快照时间 < 指定时间"),
    current_user = Depends(get_current_user),
    db: Session = Depends(get_db)
) -> Dict[str, Any]:
    """平台收益累计(余额)，即平台虚拟账户余额，的每小时快照"""
    try:
        # 构建基础查询
        query = db.query(AccProfitPlatformSnapshot)
        
        # 应用时间过滤
        if snapshot_at:
            from sqlalchemy import or_
            timestamp_filters = []
            for ts in snapshot_at:
                timestamp_filters.append(AccProfitPlatformSnapshot.snapshot_at == datetime.fromtimestamp(ts))
            query = query.filter(or_(*timestamp_filters))
        
        if snapshot_at_gte:
            query = query.filter(AccProfitPlatformSnapshot.snapshot_at >= datetime.fromtimestamp(snapshot_at_gte))
        
        if snapshot_at_lt:
            query = query.filter(AccProfitPlatformSnapshot.snapshot_at < datetime.fromtimestamp(snapshot_at_lt))
        
        # 获取总数
        total = query.count()
        
        # 应用排序（按快照时间倒序）
        query = query.order_by(desc(AccProfitPlatformSnapshot.snapshot_at))
        
        # 应用分页
        query = query.offset(offset).limit(limit)
        
        # 执行查询
        snapshots = query.all()
        
        # 格式化数据
        snapshot_list = []
        for snapshot in snapshots:
            snapshot_list.append({
                "id": snapshot.id,
                "snapshotAt": int(snapshot.snapshot_at.timestamp()),
                "accProfit": str(snapshot.acc_profit),
                "createdAt": int(snapshot.created_at.timestamp())
            })
        
        return StandardResponse.list_success(snapshot_list, total)
        
    except Exception as e:
        return StandardResponse.error(f"Failed to get platform profit snapshots: {str(e)}")


# API [33] GET /acc_profit_team
@router.get("/acc_profit_team")
async def get_acc_profit_team(
    limit: int = Query(100, description="分页，每页数量"),
    offset: int = Query(0, description="当前页面，开始的下标"),
    portfolio_id: Optional[List[int]] = Query(None, description="投资组合id(可多值)"),
    snapshot_at: Optional[List[int]] = Query(None, description="快照时间(可多值)"),
    snapshot_at_gte: Optional[int] = Query(None, description="快照时间 >= 指定时间"),
    snapshot_at_lt: Optional[int] = Query(None, description="快照时间 < 指定时间"),
    current_user = Depends(get_current_user),
    db: Session = Depends(get_db)
) -> Dict[str, Any]:
    """团队的（投资组合）累计收益(余额)，即团队虚拟账户余额，每小时快照"""
    try:
        # 构建基础查询
        query = db.query(AccProfitTeamSnapshot)
        
        # 应用投资组合过滤
        if portfolio_id:
            query = query.filter(AccProfitTeamSnapshot.portfolio_id.in_(portfolio_id))
        
        # 应用时间过滤
        if snapshot_at:
            from sqlalchemy import or_
            timestamp_filters = []
            for ts in snapshot_at:
                timestamp_filters.append(AccProfitTeamSnapshot.snapshot_at == datetime.fromtimestamp(ts))
            query = query.filter(or_(*timestamp_filters))
        
        if snapshot_at_gte:
            query = query.filter(AccProfitTeamSnapshot.snapshot_at >= datetime.fromtimestamp(snapshot_at_gte))
        
        if snapshot_at_lt:
            query = query.filter(AccProfitTeamSnapshot.snapshot_at < datetime.fromtimestamp(snapshot_at_lt))
        
        # 获取总数
        total = query.count()
        
        # 应用排序（按快照时间倒序）
        query = query.order_by(desc(AccProfitTeamSnapshot.snapshot_at))
        
        # 应用分页
        query = query.offset(offset).limit(limit)
        
        # 执行查询
        snapshots = query.all()
        
        # 格式化数据
        snapshot_list = []
        for snapshot in snapshots:
            snapshot_list.append({
                "id": snapshot.id,
                "portfolioId": snapshot.portfolio_id,
                "snapshotAt": int(snapshot.snapshot_at.timestamp()),
                "accProfit": str(snapshot.acc_profit),
                "createdAt": int(snapshot.created_at.timestamp())
            })
        
        return StandardResponse.list_success(snapshot_list, total)
        
    except Exception as e:
        return StandardResponse.error(f"Failed to get team profit snapshots: {str(e)}")