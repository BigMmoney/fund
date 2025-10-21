"""WebSocket streaming for real-time NAV updates.

Client connects to:  /ws/portfolio/nav?fund=fund/h2zl8x&frequency=hourly&interval=60

Mechanism:
  - On connect: perform initial fetch using existing get_historical_nav service
  - Store last snapshot_time (nanoseconds) or snapshot_time_str
  - Periodically (interval seconds, default 60) re-fetch a narrow window (last N hours)
  - Send only new points (where snapshot_time > last_sent_time)
  - Heartbeat ping every interval cycle even if no new data (type=heartbeat)
  - Graceful close on exceptions

Notes:
  - This avoids server-wide scheduler complexity; each connection polls upstream.
  - Upstream rate-limit mitigation: enforce minimum interval (>=30s) plus service-side cache.
  - For multi-client scaling consider central broadcaster with shared polling & pub/sub.
"""
from __future__ import annotations

from fastapi import APIRouter, WebSocket, WebSocketDisconnect, Query
from datetime import datetime, timedelta, timezone
from typing import Optional, Any
import asyncio
import math

from ...services.nav import get_historical_nav, NavServiceError

router = APIRouter()

MIN_INTERVAL = 30  # seconds
DEFAULT_INTERVAL = 60
LOOKBACK_HOURS = 48  # how much history to send initially

async def _sleep(sec: int):
    await asyncio.sleep(sec)

@router.websocket("/ws/portfolio/nav")
async def ws_portfolio_nav(
    websocket: WebSocket,
    fund: str = Query(..., description="基金标识 fund/..."),
    frequency: str = Query("hourly", pattern="^(hourly|daily)$"),
    interval: int = Query(DEFAULT_INTERVAL, ge=10, le=3600, description="刷新间隔秒(>=30更安全)"),
):
    await websocket.accept()
    if not fund.startswith("fund/"):
        await websocket.send_json({"type": "error", "message": "fund 必须以 fund/ 开头"})
        await websocket.close()
        return
    if interval < MIN_INTERVAL:
        interval = MIN_INTERVAL

    # initial window
    end_dt = datetime.now(timezone.utc)
    start_dt = end_dt - timedelta(hours=LOOKBACK_HOURS if frequency == "hourly" else 24*30)
    last_time_ns: Optional[int] = None

    async def fetch_and_diff():
        nonlocal last_time_ns, start_dt
        try:
            payload = get_historical_nav(
                fund_name=fund,
                start_time=start_dt,
                end_time=datetime.now(timezone.utc),
                frequency=frequency,
                use_cache=True,
            )
        except NavServiceError as e:
            await websocket.send_json({"type": "error", "message": str(e)})
            return
        data = payload.get("result", {}).get("historical_nav", [])
        new_points = []
        for row in data:
            snap_ns = row.get("snapshot_time")
            if snap_ns is None:
                continue
            if last_time_ns is None or snap_ns > last_time_ns:
                new_points.append(row)
        if new_points:
            # update last_time_ns to max
            last_time_ns = max(p.get("snapshot_time") for p in new_points if p.get("snapshot_time") is not None)
            await websocket.send_json({
                "type": "nav_update",
                "fund": fund,
                "frequency": frequency,
                "points": new_points,
                "latest_snapshot_time": last_time_ns,
            })
        else:
            await websocket.send_json({"type": "heartbeat", "fund": fund, "frequency": frequency})

    # Initial full push (as nav_update for uniform handling)
    try:
        payload = get_historical_nav(
            fund_name=fund,
            start_time=start_dt,
            end_time=end_dt,
            frequency=frequency,
            use_cache=True,
        )
        rows = payload.get("result", {}).get("historical_nav", [])
        if rows:
            last_time_ns = max(r.get("snapshot_time") for r in rows if r.get("snapshot_time") is not None)
        await websocket.send_json({
            "type": "nav_snapshot",
            "fund": fund,
            "frequency": frequency,
            "points": rows,
            "latest_snapshot_time": last_time_ns,
        })
    except NavServiceError as e:
        await websocket.send_json({"type": "error", "message": str(e)})
        await websocket.close()
        return

    try:
        while True:
            await _sleep(interval)
            await fetch_and_diff()
    except WebSocketDisconnect:
        # client disconnected
        return
    except Exception as e:  # pragma: no cover
        try:
            await websocket.send_json({"type": "error", "message": f"内部错误: {e}"})
        finally:
            await websocket.close()
