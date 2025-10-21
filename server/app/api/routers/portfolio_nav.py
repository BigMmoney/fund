"""Frontend NAV aggregation endpoint.

Route: GET /api/frontend/portfolio/nav

Parameters:
  fund: fund/h2zl8x (required)
  frequency: daily|hourly (default: daily)
  days: integer window (mutually exclusive with start/end)
  start: ISO8601 start (optional)
  end: ISO8601 end (optional, defaults now UTC)

Behavior:
  - Builds time range (days has priority if provided)
  - Calls upstream via nav.get_historical_nav with cache
  - Normalizes records and adds derived metrics:
        hourly/daily return, drawdown, max_drawdown, ranges summary
  - Returns compact JSON suited for charting.
"""
from __future__ import annotations

from fastapi import APIRouter, HTTPException, Query
from typing import Optional, List, Dict, Any
from datetime import datetime, timedelta, timezone

from ...services.nav import get_historical_nav, NavServiceError

router = APIRouter()

def _parse_iso(ts: str) -> datetime:
    # Accept 'YYYY-MM-DDTHH:MM:SSZ' or without Z
    if ts.endswith("Z"):
        ts = ts[:-1]
    # Allow missing seconds
    try:
        dt = datetime.fromisoformat(ts)
    except ValueError:
        raise HTTPException(status_code=422, detail=f"时间格式非法: {ts}")
    if dt.tzinfo is None:
        dt = dt.replace(tzinfo=timezone.utc)
    return dt.astimezone(timezone.utc)

def _compute_metrics(items: List[Dict[str, Any]]) -> Dict[str, Any]:
    if not items:
        return {"points": 0}
    # accum_nav numeric values
    acc = [float(x["accum_nav"]) for x in items]
    na = [float(x["net_assets"]) for x in items]
    first, last = acc[0], acc[-1]
    change = (last / first - 1) if first else 0
    max_nav = max(acc)
    min_nav = min(acc)
    # Drawdown sequence
    peak = acc[0]
    drawdowns = []
    for v in acc:
        if v > peak:
            peak = v
        dd = (v / peak - 1) if peak else 0
        drawdowns.append(dd)
    max_drawdown = min(drawdowns) if drawdowns else 0
    # Returns (period over previous point)
    rets = []
    for i in range(1, len(acc)):
        prev = acc[i-1]
        if prev:
            rets.append(acc[i]/prev - 1)
    avg_ret = sum(rets)/len(rets) if rets else 0
    # Simple volatility (stdev) if at least 2
    vol = 0
    if len(rets) > 1:
        mean = avg_ret
        var = sum((r-mean)**2 for r in rets)/(len(rets)-1)
        vol = var ** 0.5
    return {
        "points": len(items),
        "accum_nav_first": first,
        "accum_nav_last": last,
        "accum_nav_change": change,
        "accum_nav_range": [min_nav, max_nav],
        "net_assets_range": [min(na), max(na)],
        "max_drawdown": max_drawdown,
        "avg_point_return": avg_ret,
        "return_volatility": vol,
    }

@router.get("/portfolio/nav")
def portfolio_nav(
    fund: str = Query(..., description="基金标识，含前缀 fund/"),
    frequency: str = Query("daily", pattern="^(daily|hourly)$"),
    days: Optional[int] = Query(None, ge=1, le=365, description="向前回溯天数 (与 start/end 互斥)"),
    start: Optional[str] = Query(None, description="起始时间 ISO8601"),
    end: Optional[str] = Query(None, description="结束时间 ISO8601，缺省为当前 UTC"),
    use_cache: bool = Query(True, description="是否使用内部缓存"),
):
    if not fund.startswith("fund/"):
        raise HTTPException(status_code=422, detail="fund 参数必须以 'fund/' 开头")

    now = datetime.now(timezone.utc)
    if end:
        end_dt = _parse_iso(end)
    else:
        end_dt = now

    if days is not None and (start or end):
        raise HTTPException(status_code=422, detail="days 与 start/end 互斥，请二选一")

    if days is not None:
        start_dt = end_dt - timedelta(days=days)
    else:
        if start:
            start_dt = _parse_iso(start)
        else:
            # default window: 7 days for hourly, 30 days for daily
            default_days = 7 if frequency == "hourly" else 30
            start_dt = end_dt - timedelta(days=default_days)

    if start_dt >= end_dt:
        raise HTTPException(status_code=422, detail="start 必须早于 end")

    try:
        upstream = get_historical_nav(
            fund_name=fund,
            start_time=start_dt,
            end_time=end_dt,
            frequency=frequency,
            use_cache=use_cache,
        )
    except NavServiceError as e:
        raise HTTPException(status_code=502, detail=str(e))
    except Exception as e:  # pragma: no cover
        raise HTTPException(status_code=500, detail=f"内部错误: {e}")

    data = upstream.get("result", {}).get("historical_nav", [])
    # Normalize & derive returns/drawdown per point
    normalized = []
    prev_accum = None
    peak_accum = None
    for row in data:
        accum = float(row.get("accum_nav", 0)) if row.get("accum_nav") is not None else None
        net_assets = float(row.get("net_assets", 0)) if row.get("net_assets") is not None else None
        if peak_accum is None or (accum is not None and accum > peak_accum):
            peak_accum = accum
        ret = None
        dd = None
        if accum is not None:
            if prev_accum not in (None, 0):
                ret = accum / prev_accum - 1
            if peak_accum not in (None, 0):
                dd = accum / peak_accum - 1
        normalized.append({
            "time": row.get("snapshot_time_str"),
            "net_assets": net_assets,
            "accum_nav": accum,
            "accum_pnl": float(row.get("accum_pnl", 0)) if row.get("accum_pnl") is not None else None,
            "return": ret,
            "drawdown": dd,
        })
        prev_accum = accum

    metrics = _compute_metrics(data)

    return {
        "fund": fund,
        "frequency": frequency,
        "window": {
            "start": start_dt.strftime("%Y-%m-%dT%H:%M:%SZ"),
            "end": end_dt.strftime("%Y-%m-%dT%H:%M:%SZ"),
            "days": (end_dt - start_dt).days + ((end_dt - start_dt).seconds > 0),
        },
        "metrics": metrics,
        "points": normalized,
        "source": {"cached": upstream is not None and use_cache},
    }
