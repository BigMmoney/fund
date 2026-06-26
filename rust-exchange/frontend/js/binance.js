// js/binance.js — BinanceProvider
//
// Single source of all Binance public market-data interactions used by the
// Trade page (`/#user/trade`). Encapsulates REST + WebSocket plumbing for
// klines, ticker, depth (order book), and trades, with auto-reconnect on
// every WS subscription.
//
// Hosts:
//   REST: https://data-api.binance.vision  (Binance public-data mirror;
//         reachable from regions where api.binance.com is geo-blocked,
//         e.g. mainland China returns HTTP 451 on api.binance.com but 200
//         on data-api.binance.vision)
//   WS:   wss://data-stream.binance.vision/ws/{symbol}@<stream>
//
// Override at runtime with `window.__BINANCE_REST` / `window.__BINANCE_WS`
// (set in index.html before the modules load) if a different host is
// required.

import { escapeHtml } from './core.js';

// REST goes through the local api at /binance/rest/* (which proxies to
// data-api.binance.vision). This sidesteps CORS variability, browser
// extensions, AV intercepts, and any geo-block that affects browser DNS
// but not the host's. WS still talks to Binance directly — WebSocket
// handshakes are not subject to CORS, so the proxy isn't needed there.
//
// Override either at runtime by setting window.__BINANCE_REST /
// window.__BINANCE_WS in index.html before the modules load.
const REST_BASE = (typeof window !== 'undefined' && window.__BINANCE_REST) || '/binance/rest';
const WS_BASE   = (typeof window !== 'undefined' && window.__BINANCE_WS)   || 'wss://data-stream.binance.vision/ws';

// Module-load marker. Confirms which version + URLs the browser actually
// loaded — invaluable when debugging cache issues.
try { console.log('[binance] module loaded', { REST_BASE, WS_BASE, build: '2026-05-06c-proxy' }); } catch {}

// UI market id → Binance symbol. Falls back to dash-strip + upper-case.
const SYMBOL_MAP = {
  'btc-usdt':  'BTCUSDT',
  'eth-usdt':  'ETHUSDT',
  'usdc-usdt': 'USDCUSDT',
};
// Seconds → Binance interval token.
const INTERVAL_MAP = {
  60:    '1m',
  180:   '3m',
  300:   '5m',
  900:   '15m',
  1800:  '30m',
  3600:  '1h',
  7200:  '2h',
  14400: '4h',
  21600: '6h',
  43200: '12h',
  86400: '1d',
};

const RECONNECT_BACKOFF = [1000, 2000, 5000, 10000, 30000];

export function toBinanceSymbol(market) {
  return SYMBOL_MAP[market] || String(market || '').replace(/[^a-zA-Z0-9]/g, '').toUpperCase();
}
export function toBinanceInterval(sec) {
  return INTERVAL_MAP[sec] || '1m';
}

// ─── REST ────────────────────────────────────────────────────────

// Returns [{t,o,h,l,c,v}] sorted ascending. `t` is seconds (matches our candle shape).
export async function fetchBinanceKlines(market, intervalSec, limit = 500) {
  const sym = toBinanceSymbol(market);
  const ival = toBinanceInterval(intervalSec);
  const url = `${REST_BASE}/api/v3/klines?symbol=${encodeURIComponent(sym)}&interval=${encodeURIComponent(ival)}&limit=${Math.min(1000, Math.max(1, limit))}`;
  const arr = await fetchJson(url, 'klines');
  return arr.map(k => ({
    t: Math.floor(k[0] / 1000),
    o: parseFloat(k[1]),
    h: parseFloat(k[2]),
    l: parseFloat(k[3]),
    c: parseFloat(k[4]),
    v: parseFloat(k[5]),
  }));
}

export async function fetchBinanceTicker(market) {
  const sym = toBinanceSymbol(market);
  const url = `${REST_BASE}/api/v3/ticker/24hr?symbol=${encodeURIComponent(sym)}`;
  const r = await fetchJson(url, '24hr ticker');
  return {
    symbol:        r.symbol,
    last_price:    parseFloat(r.lastPrice),
    price_change:  parseFloat(r.priceChange),
    price_change_percent: parseFloat(r.priceChangePercent),
    high_price:    parseFloat(r.highPrice),
    low_price:     parseFloat(r.lowPrice),
    volume:        parseFloat(r.volume),
    quote_volume:  parseFloat(r.quoteVolume),
    open_time:     Number(r.openTime),
    close_time:    Number(r.closeTime),
  };
}

// Returns { bids:[{price, quantity}], asks:[{price, quantity}], lastUpdateId }
// — shaped to match the local engine's /markets/{m}/book response so the
// existing renderer in pages-user.js can consume it unchanged.
export async function fetchBinanceDepth(market, limit = 20) {
  const sym = toBinanceSymbol(market);
  const url = `${REST_BASE}/api/v3/depth?symbol=${encodeURIComponent(sym)}&limit=${limit}`;
  const r = await fetchJson(url, 'depth');
  return {
    lastUpdateId: Number(r.lastUpdateId ?? 0),
    bids: (r.bids || []).map(([p, q]) => ({ price: parseFloat(p), quantity: parseFloat(q) })),
    asks: (r.asks || []).map(([p, q]) => ({ price: parseFloat(p), quantity: parseFloat(q) })),
  };
}

// Returns [{ price, quantity, side, timestamp }] — local-engine-shaped so
// existing trade-tape rendering in pages-user.js works unchanged.
export async function fetchBinanceTrades(market, limit = 30) {
  const sym = toBinanceSymbol(market);
  const url = `${REST_BASE}/api/v3/trades?symbol=${encodeURIComponent(sym)}&limit=${limit}`;
  const arr = await fetchJson(url, 'trades');
  // Most-recent first.
  return arr.slice().reverse().map(t => ({
    price:     parseFloat(t.price),
    quantity:  parseFloat(t.qty),
    side:      t.isBuyerMaker ? 'sell' : 'buy',
    timestamp: new Date(Number(t.time)).toISOString(),
  }));
}

async function fetchJson(url, kind) {
  console.log('[binance] REST', kind, url);
  let r;
  try {
    r = await fetch(url, { headers: { accept: 'application/json' } });
  } catch (e) {
    console.error('[binance] REST fetch threw', kind, url, e);
    throw new Error(`binance ${kind}: ${e?.message || 'fetch failed'}`);
  }
  if (!r.ok) {
    let msg = `binance ${kind} HTTP ${r.status}`;
    try { msg += ' — ' + (await r.text()).slice(0, 160); } catch {}
    console.error('[binance] REST non-2xx', kind, url, r.status, msg);
    throw new Error(msg);
  }
  return r.json();
}

// ─── WebSocket connection (auto-reconnect) ───────────────────────

// Internal helper: open a WS to a single Binance stream and reconnect on
// close/error with exponential backoff. The caller's onMessage receives
// already-parsed JSON; non-JSON frames are silently dropped.
class WsConnection {
  constructor(stream, onMessage, onStatus) {
    this.stream = stream;
    this.onMessage = onMessage || (() => {});
    this.onStatus = onStatus || (() => {});
    this._ws = null;
    this._closed = false;
    this._reconnectIdx = 0;
    this._reconnectTimer = null;
  }
  start() {
    this._closed = false;
    this._connect();
  }
  close() {
    this._closed = true;
    if (this._reconnectTimer) { clearTimeout(this._reconnectTimer); this._reconnectTimer = null; }
    const ws = this._ws; this._ws = null;
    if (ws) { try { ws.onopen = ws.onmessage = ws.onerror = ws.onclose = null; ws.close(); } catch {} }
  }
  _connect() {
    if (this._closed) return;
    const wsUrl = `${WS_BASE}/${this.stream}`;
    console.log('[binance] WS connect', wsUrl);
    let ws;
    try { ws = new WebSocket(wsUrl); }
    catch (e) {
      console.error('[binance] WS construct threw', wsUrl, e);
      this.onStatus({ kind: 'error', error: e?.message }); this._scheduleReconnect(); return;
    }
    this._ws = ws;
    this.onStatus({ kind: 'connecting', stream: this.stream });
    ws.onopen = () => {
      this._reconnectIdx = 0;
      console.log('[binance] WS open', wsUrl);
      this.onStatus({ kind: 'open', stream: this.stream, source: 'binance' });
    };
    ws.onmessage = (ev) => {
      let m;
      try { m = JSON.parse(ev.data); } catch { return; }
      try { this.onMessage(m); } catch (e) {
        // Surface but don't crash the loop.
        console.error('[binance] message handler error', e);
      }
    };
    ws.onerror = (ev) => {
      console.error('[binance] WS error', wsUrl, ev);
      this.onStatus({ kind: 'error', stream: this.stream });
    };
    ws.onclose = (ev) => {
      console.log('[binance] WS close', wsUrl, { code: ev.code, reason: ev.reason, wasClean: ev.wasClean });
      this._ws = null;
      if (this._closed) return;
      this._scheduleReconnect();
    };
  }
  _scheduleReconnect() {
    if (this._closed || this._reconnectTimer) return;
    const delay = RECONNECT_BACKOFF[Math.min(this._reconnectIdx, RECONNECT_BACKOFF.length - 1)];
    this._reconnectIdx++;
    this.onStatus({ kind: 'reconnect', stream: this.stream, in_ms: delay, attempt: this._reconnectIdx });
    this._reconnectTimer = setTimeout(() => { this._reconnectTimer = null; this._connect(); }, delay);
  }
}

// Live kline stream. Emits one candle per server tick; the last candle of
// the active bucket is updated in place until the bucket closes (k.x ===
// true), after which a new bucket starts.
export class BinanceKlineStream {
  constructor(market, intervalSec, { onCandle, onStatus } = {}) {
    const sym = toBinanceSymbol(market).toLowerCase();
    const ival = toBinanceInterval(intervalSec);
    this._conn = new WsConnection(`${sym}@kline_${ival}`, (m) => {
      const k = m.k; if (!k) return;
      onCandle?.({
        t: Math.floor(k.t / 1000),
        o: parseFloat(k.o),
        h: parseFloat(k.h),
        l: parseFloat(k.l),
        c: parseFloat(k.c),
        v: parseFloat(k.v),
        closed: !!k.x,
      });
    }, onStatus);
  }
  start() { this._conn.start(); }
  close() { this._conn.close(); }
}

// Live ticker stream. Emits a normalised TickerRow per tick.
export class BinanceTickerStream {
  constructor(market, { onTicker, onStatus } = {}) {
    const sym = toBinanceSymbol(market).toLowerCase();
    this._conn = new WsConnection(`${sym}@ticker`, (r) => {
      onTicker?.({
        symbol:        r.s,
        last_price:    parseFloat(r.c),
        price_change:  parseFloat(r.p),
        price_change_percent: parseFloat(r.P),
        high_price:    parseFloat(r.h),
        low_price:     parseFloat(r.l),
        volume:        parseFloat(r.v),
        quote_volume:  parseFloat(r.q),
      });
    }, onStatus);
  }
  start() { this._conn.start(); }
  close() { this._conn.close(); }
}

// Live order-book stream (top N levels every speedMs).
// Emits { bids:[{price, quantity}], asks:[{price, quantity}] } shaped to
// match the local /markets/{m}/book endpoint so the existing renderer
// in pages-user.js can consume it unchanged.
export class BinanceDepthStream {
  constructor(market, { onDepth, onStatus, levels = 20, speedMs = 100 } = {}) {
    const sym = toBinanceSymbol(market).toLowerCase();
    if (![5, 10, 20].includes(levels)) levels = 20;
    if (![100, 1000].includes(speedMs)) speedMs = 100;
    this._conn = new WsConnection(`${sym}@depth${levels}@${speedMs}ms`, (r) => {
      onDepth?.({
        lastUpdateId: Number(r.lastUpdateId ?? 0),
        bids: (r.bids || []).map(([p, q]) => ({ price: parseFloat(p), quantity: parseFloat(q) })),
        asks: (r.asks || []).map(([p, q]) => ({ price: parseFloat(p), quantity: parseFloat(q) })),
      });
    }, onStatus);
  }
  start() { this._conn.start(); }
  close() { this._conn.close(); }
}

// Live trades stream — emits one trade per execution.
export class BinanceTradeStream {
  constructor(market, { onTrade, onStatus } = {}) {
    const sym = toBinanceSymbol(market).toLowerCase();
    this._conn = new WsConnection(`${sym}@trade`, (r) => {
      onTrade?.({
        price:     parseFloat(r.p),
        quantity:  parseFloat(r.q),
        side:      r.m ? 'sell' : 'buy', // m=true means buyer is maker, i.e. taker sold
        timestamp: new Date(Number(r.T)).toISOString(),
        trade_id:  Number(r.t),
      });
    }, onStatus);
  }
  start() { this._conn.start(); }
  close() { this._conn.close(); }
}

// ─── Friendly error formatter (used by callers in pages-user.js) ──
export function formatBinanceError(e) {
  return escapeHtml(e?.message || String(e || 'unknown error'));
}
