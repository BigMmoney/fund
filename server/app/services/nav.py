"""NAV (Historical Net Asset Value) upstream service.

This module encapsulates calling the upstream 1Token endpoint:
POST /api/v1/fundv3/openapi/portfolio/get-historical-nav

Signature pattern (validated via probe scripts):
    message = verb + path + timestamp + compact_json_body
    signature = base64( HMAC_SHA256( base64_decode(secret), message ) )
Headers:
    Api-Key
    Api-Signature
    Api-Timestamp  (seconds integer, UTC epoch)
    Content-Type: application/json

This service provides a single convenience entry point:
    get_historical_nav(fund_name, start_time, end_time, frequency="daily")

It accepts datetime or string ISO8601 inputs and normalizes them.

NOTE: This file intentionally does NOT reuse the existing OneTokenClient because
that client uses a different header scheme (X-ONETOKEN-*) which did not work for
this particular private endpoint during reverse‑engineering. Keeping a focused
implementation avoids accidental regression.
"""
from __future__ import annotations

import base64
import hmac
import hashlib
import json
import time
from datetime import datetime, timezone
from typing import Any, Dict, Union

import requests
import logging

from server.app.settings import settings
import os

logger = logging.getLogger(__name__)

# ---------------- In-memory TTL cache (simple) -----------------
# Key: (fund_name, start_iso, end_iso, frequency)
# Value: (expire_epoch_seconds, payload_dict)
_NAV_CACHE: dict[tuple[str, str, str, str], tuple[int, dict]] = {}

def _cache_get(key: tuple[str, str, str, str]) -> dict | None:
    rec = _NAV_CACHE.get(key)
    if not rec:
        return None
    expire, data = rec
    if expire < time.time():  # expired
        _NAV_CACHE.pop(key, None)
        return None
    return data

def _cache_set(key: tuple[str, str, str, str], data: dict, ttl: int) -> None:
    _NAV_CACHE[key] = (int(time.time()) + ttl, data)

# Primary upstream path (without /api/v1 since it's in base URL now)
PRIMARY_UPSTREAM_PATH = "/fundv3/openapi/portfolio/get-historical-nav"
FALLBACK_UPSTREAM_PATH = "/api/v1/fundv3/openapi/portfolio/get-historical-nav"  # Keep for backup
# We keep old name for minimal downstream change; will dynamically decide which to use on first successful call.
UPSTREAM_PATH = PRIMARY_UPSTREAM_PATH
_resolved_upstream_path: str | None = None
ALLOWED_FREQUENCIES = {"daily", "hourly"}

class NavServiceError(RuntimeError):
    """Raised when the upstream NAV call fails or returns an error code."""


def _coerce_datetime(ts: Union[str, datetime]) -> datetime:
    """Convert input (ISO8601 string or datetime) to aware UTC datetime."""
    if isinstance(ts, datetime):
        return ts.astimezone(timezone.utc) if ts.tzinfo else ts.replace(tzinfo=timezone.utc)
    # string
    s = ts.rstrip("Z")
    dt = datetime.fromisoformat(s)
    if dt.tzinfo is None:
        dt = dt.replace(tzinfo=timezone.utc)
    return dt.astimezone(timezone.utc)

def _to_ns(dt: datetime) -> int:
    return int(dt.timestamp() * 1_000_000_000)

def _to_iso(dt: datetime) -> str:
    return dt.strftime("%Y-%m-%dT%H:%M:%SZ")


def _compact_json(data: Dict[str, Any]) -> str:
    return json.dumps(data, separators=(",", ":"), ensure_ascii=False)


def _sign(secret_b64: str, verb: str, path: str, ts: int, body: Dict[str, Any]) -> str:
    # For signature: use seconds timestamp + standard JSON body (not compact)
    message = f"{verb}{path}{ts}{json.dumps(body)}"
    raw = hmac.new(base64.b64decode(secret_b64), message.encode("utf-8"), hashlib.sha256).digest()
    return base64.b64encode(raw).decode("utf-8")


def _collect_key_pairs() -> list[tuple[str,str]]:
    pairs: list[tuple[str,str]] = []
    # Primary single vars
    if settings.ONETOKEN_API_KEY and settings.ONETOKEN_SECRET:
        pairs.append((settings.ONETOKEN_API_KEY, settings.ONETOKEN_SECRET))
    # Numbered fallback vars: ONETOKEN_API_KEY_1 / ONETOKEN_SECRET_1 etc.
    for i in range(1,6):  # support up to 5 slots
        k = os.getenv(f"ONETOKEN_API_KEY_{i}")
        s = os.getenv(f"ONETOKEN_SECRET_{i}") or os.getenv(f"ONETOKEN_API_SECRET_{i}")
        if k and s:
            candidate = (k, s)
            if candidate not in pairs:
                pairs.append(candidate)
    return pairs


def get_historical_nav(
    fund_name: str,
    start_time: Union[str, datetime],
    end_time: Union[str, datetime],
    frequency: str = "daily",
    timeout: int = 30,
    cache_ttl: int | None = 120,
    use_cache: bool = True,
) -> Dict[str, Any]:
    """Fetch historical NAV from upstream.

    Parameters
    ----------
    fund_name : str
        Must include the 'fund/' prefix (validated upstream).
    start_time, end_time : str | datetime
        Time window (UTC). Datetimes converted to ISO8601 with 'Z'.
    frequency : str
        One of 'daily', 'hourly'.
    timeout : int
        Request timeout in seconds.

    Returns
    -------
    dict
        Parsed JSON response. On success contains result.historical_nav list.

    Raises
    ------
    NavServiceError
        If HTTP not 200, network error, or upstream code indicates problem.
    """
    if frequency not in ALLOWED_FREQUENCIES:
        raise ValueError(f"Unsupported frequency '{frequency}', allowed: {ALLOWED_FREQUENCIES}")

    # Convert to datetime then derive nanosecond timestamps (spec requires ns integers)
    start_dt = _coerce_datetime(start_time)
    end_dt = _coerce_datetime(end_time)
    start_iso = _to_iso(start_dt)
    end_iso = _to_iso(end_dt)
    start_ns = _to_ns(start_dt)
    end_ns = _to_ns(end_dt)

    cache_key = (fund_name, start_iso, end_iso, frequency)
    if use_cache and cache_ttl and cache_ttl > 0:
        cached = _cache_get(cache_key)
        if cached is not None:
            return cached

    body = {
        "fund_name": fund_name,
        "start_time": start_ns,
        "end_time": end_ns,
        "frequency": frequency,
    }
    
    debug_enabled = os.getenv("NAV_DEBUG") == "1"
    if debug_enabled:
        logger.info("NAV request body ns range %s -> %s frequency=%s", start_ns, end_ns, frequency)

    global _resolved_upstream_path, UPSTREAM_PATH
    # Determine which paths to try
    if _resolved_upstream_path:
        path_candidates = [_resolved_upstream_path]
    else:
        path_candidates = [PRIMARY_UPSTREAM_PATH, FALLBACK_UPSTREAM_PATH]

    key_pairs = _collect_key_pairs()
    if not key_pairs:
        raise NavServiceError("No OneToken credentials configured (ONETOKEN_API_KEY / SECRET).")

    last_error: str | None = None

    for path_option in path_candidates:
        full_url = f"{settings.ONETOKEN_BASE_URL}{path_option}"
        if debug_enabled:
            logger.info("NAV using path candidate: %s", path_option)
        for idx, (api_key, secret) in enumerate(key_pairs, start=1):
            ts = int(time.time())
            try:
                # Use standard JSON format for signature (not compact)
                signature = _sign(secret, "POST", path_option, ts, body)
            except Exception as e:  # pragma: no cover
                last_error = f"sign error: {e}"
                if debug_enabled:
                    logger.warning("NAV attempt %d sign error: %s", idx, e)
                continue
            headers = {
                "Api-Key": api_key,
                "Api-Signature": signature,
                "Api-Timestamp": str(ts),
                "Content-Type": "application/json",
            }
            try:
                # Send request with standard JSON (not compact)
                resp = requests.post(full_url, data=json.dumps(body), headers=headers, timeout=timeout)
            except Exception as e:  # pragma: no cover
                last_error = f"network error: {e}"
                if debug_enabled:
                    logger.warning("NAV attempt %d network error: %s", idx, e)
                continue
            if resp.status_code == 404 and not _resolved_upstream_path and path_option == PRIMARY_UPSTREAM_PATH:
                last_error = "primary path 404, will try fallback"
                if debug_enabled:
                    logger.warning("NAV primary path 404, switching to fallback once")
                break  # break inner loop -> next path_option
            if resp.status_code == 401:
                last_error = f"auth failed with provided key (ending {api_key[-6:]})"
                if debug_enabled:
                    logger.warning("NAV attempt %d 401 auth failure key_end=%s body=%s", idx, api_key[-6:], resp.text[:300])
                continue
            if resp.status_code != 200:
                last_error = f"HTTP {resp.status_code} {resp.text[:120]}"
                if debug_enabled:
                    logger.warning(
                        "NAV attempt %d HTTP %s key_end=%s snippet=%s", idx, resp.status_code, api_key[-6:], resp.text[:300]
                    )
                continue
            try:
                payload = resp.json()
            except ValueError as e:
                last_error = f"invalid json: {e}"
                if debug_enabled:
                    logger.warning("NAV attempt %d invalid json: %s body=%s", idx, e, resp.text[:200])
                continue
            code = payload.get("code", "")
            if code not in ("", 0, "0"):
                last_error = f"upstream code={code} message={payload.get('message')}"
                if debug_enabled:
                    logger.warning("NAV attempt %d upstream logical error code=%s msg=%s", idx, code, payload.get("message"))
                continue
            if debug_enabled:
                logger.info(
                    "NAV success attempt %d path=%s points=%s", idx, path_option, len(payload.get("result", {}).get("historical_nav", []))
                )
            if _resolved_upstream_path is None:
                _resolved_upstream_path = path_option
                UPSTREAM_PATH = path_option
            if use_cache and cache_ttl and cache_ttl > 0:
                _cache_set(cache_key, payload, cache_ttl)
            return payload
        else:
            # inner loop exhausted without success, continue to next path_option
            continue
        # hit break due to 404 fallback trigger
        continue

    raise NavServiceError(last_error or "All credential attempts failed")


# Convenience alias for external import
__all__ = ["get_historical_nav", "NavServiceError"]
