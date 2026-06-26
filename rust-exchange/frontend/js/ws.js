// js/ws.js — WebSocket trade-stream client for /ws/trades/:market_id.
// Falls back to REST polling if the upgrade fails or closes early.

const RECONNECT_BACKOFF = [1000, 2000, 5000, 10000, 30000];
const POLL_INTERVAL_MS = 5000;

export class TradeStream {
  constructor(market, { onTrade, onStatus, base, fallbackPollMs = POLL_INTERVAL_MS } = {}) {
    this.market = market;
    this.onTrade = onTrade || (() => {});
    this.onStatus = onStatus || (() => {});
    this.base = base || location.origin;
    this.fallbackPollMs = fallbackPollMs;
    this._closed = false;
    this._ws = null;
    this._reconnectIdx = 0;
    this._reconnectTimer = null;
    this._pollTimer = null;
    // Bounded set of recently-seen trade ids — used by both WS and the polling
    // fallback to dedup. Set+queue gives O(1) insert/lookup with a hard cap.
    this._seenIds = new Set();
    this._seenQueue = [];
    this._seenCap = 1024;
    // Watermark for trades without a stable id (some providers omit it).
    this._lastTimestampMs = 0;
  }
  _markSeen(id) {
    if (!id) return;
    this._seenIds.add(id);
    this._seenQueue.push(id);
    if (this._seenQueue.length > this._seenCap) {
      const drop = this._seenQueue.shift();
      this._seenIds.delete(drop);
    }
  }
  start() {
    this._closed = false;
    this._connect();
  }
  close() {
    this._closed = true;
    this._teardownWs();
    if (this._pollTimer) { clearInterval(this._pollTimer); this._pollTimer = null; }
    if (this._reconnectTimer) { clearTimeout(this._reconnectTimer); this._reconnectTimer = null; }
  }
  _wsUrl() {
    const u = new URL(this.base);
    const proto = u.protocol === 'https:' ? 'wss:' : 'ws:';
    return `${proto}//${u.host}/ws/trades/${encodeURIComponent(this.market)}`;
  }
  _connect() {
    if (this._closed) return;
    let ws;
    try {
      ws = new WebSocket(this._wsUrl());
    } catch (e) {
      this._fallbackToPolling('WS construction failed: ' + e.message);
      return;
    }
    this._ws = ws;
    let openedFor = 0;
    const openedAt = Date.now();
    this.onStatus({ kind: 'connecting', market: this.market });
    ws.onopen = () => {
      this._reconnectIdx = 0; // reset backoff on successful connect
      if (this._pollTimer) { clearInterval(this._pollTimer); this._pollTimer = null; } // stop fallback poll
      this.onStatus({ kind: 'open', market: this.market });
    };
    ws.onmessage = (ev) => {
      let msg;
      try { msg = JSON.parse(ev.data); } catch { return; }
      if (msg.event_type === 'warning' || msg.type === 'warning') return;
      const d = msg.data || msg;
      if (!d || d.timestamp == null) return;
      // Dedup: id-based when present, timestamp-watermark otherwise.
      if (d.trade_id && this._seenIds.has(d.trade_id)) return;
      const tsMs = Date.parse(d.timestamp);
      if (!d.trade_id && !isNaN(tsMs) && tsMs <= this._lastTimestampMs) return;
      this._markSeen(d.trade_id);
      if (!isNaN(tsMs)) this._lastTimestampMs = Math.max(this._lastTimestampMs, tsMs);
      this.onTrade({
        trade_id: d.trade_id,
        price: d.price,
        quantity: d.amount ?? d.quantity,
        side: (d.side || '').toLowerCase().replace(/^"|"$/g, ''),
        timestamp: d.timestamp,
      });
    };
    ws.onerror = () => {
      // browsers fire close after error; let onclose handle reconnect.
      this.onStatus({ kind: 'error', market: this.market });
    };
    ws.onclose = (ev) => {
      this._ws = null;
      openedFor = Date.now() - openedAt;
      if (this._closed) return;
      // If the connection died immediately (< 1500ms) on the first attempt, the server
      // probably doesn't support WS for this path — fall back to polling rather than
      // burning into the reconnect backoff.
      if (this._reconnectIdx === 0 && openedFor < 1500) {
        this._fallbackToPolling(`WS closed early (${ev.code}) — falling back to polling`);
        return;
      }
      const delay = RECONNECT_BACKOFF[Math.min(this._reconnectIdx, RECONNECT_BACKOFF.length - 1)];
      this._reconnectIdx++;
      this.onStatus({ kind: 'reconnect', market: this.market, in_ms: delay, attempt: this._reconnectIdx });
      // Run the fallback poll while we wait for the next reconnect — keeps the chart fresh.
      this._startPolling();
      this._reconnectTimer = setTimeout(() => { this._reconnectTimer = null; this._connect(); }, delay);
    };
  }
  _teardownWs() {
    const ws = this._ws; this._ws = null;
    if (ws) {
      try { ws.onopen = ws.onmessage = ws.onerror = ws.onclose = null; ws.close(); } catch {}
    }
  }
  _fallbackToPolling(reason) {
    this.onStatus({ kind: 'fallback', market: this.market, reason });
    this._startPolling();
  }
  _startPolling() {
    if (this._pollTimer || this._closed) return;
    const tick = async () => {
      if (this._closed) return;
      try {
        const r = await fetch(`${this.base.replace(/\/$/,'')}/markets/${encodeURIComponent(this.market)}/trades`);
        if (!r.ok) return;
        const j = await r.json();
        const arr = Array.isArray(j) ? j : (j.trades || []);
        // Replay oldest → newest so candle aggregation order is consistent.
        for (const t of arr.slice().reverse()) {
          if (t.trade_id && this._seenIds.has(t.trade_id)) continue;
          const tsMs = Date.parse(t.timestamp);
          if (!t.trade_id && !isNaN(tsMs) && tsMs <= this._lastTimestampMs) continue;
          this._markSeen(t.trade_id);
          if (!isNaN(tsMs)) this._lastTimestampMs = Math.max(this._lastTimestampMs, tsMs);
          this.onTrade(t);
        }
      } catch {}
    };
    this._pollTimer = setInterval(tick, this.fallbackPollMs);
    tick();
  }
}
