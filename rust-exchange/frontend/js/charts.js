// js/charts.js — donut + K-line chart engines.
// loadKline opens a TradeStream and updates the latest candle on every fill,
// avoiding the polling loop entirely once the WS upgrade succeeds.

import { $, escapeHtml, get } from './core.js';
import { TradeStream } from './ws.js';
import { fetchBinanceKlines, BinanceKlineStream } from './binance.js';

// ─── K-line aggregation + render ───────────────────────────────────
export function aggregateTrades(trades, intervalSec) {
  if (!trades || !trades.length) return [];
  const buckets = new Map();
  for (const t of trades) {
    const ts = Date.parse(t.timestamp);
    if (isNaN(ts)) continue;
    const bucket = Math.floor(ts / 1000 / intervalSec) * intervalSec;
    const px = parseFloat(t.price);
    const qty = parseFloat(t.quantity || t.qty || 0);
    if (!buckets.has(bucket)) buckets.set(bucket, { t: bucket, o: px, h: px, l: px, c: px, v: 0 });
    const b = buckets.get(bucket);
    b.h = Math.max(b.h, px); b.l = Math.min(b.l, px); b.c = px; b.v += qty;
  }
  return [...buckets.values()].sort((a,b) => a.t - b.t);
}

// Mutates `candles` to fold a single new trade into the appropriate bucket
// (extending the latest candle or opening a new one). Returns true if a new
// bucket was opened so callers can decide whether to fully re-paint vs update.
export function foldTradeIntoCandles(candles, trade, intervalSec) {
  const tsMs = Date.parse(trade.timestamp);
  if (isNaN(tsMs)) return false;
  const bucket = Math.floor(tsMs / 1000 / intervalSec) * intervalSec;
  const px = parseFloat(trade.price);
  const qty = parseFloat(trade.quantity || trade.qty || 0);
  const last = candles[candles.length - 1];
  if (last && last.t === bucket) {
    last.h = Math.max(last.h, px); last.l = Math.min(last.l, px); last.c = px; last.v += qty;
    return false;
  }
  if (last && bucket < last.t) return false; // out-of-order trade older than tail
  candles.push({ t: bucket, o: px, h: px, l: px, c: px, v: qty });
  return true;
}

export function renderKline(host, candles) {
  const W = host.clientWidth || 600, H = host.clientHeight || 300;
  const padL = 50, padR = 8, padT = 12, padB = 24;
  const innerW = W - padL - padR, innerH = H - padT - padB;
  const volH = Math.floor(innerH * 0.18);
  const priceH = innerH - volH - 6;
  if (candles.length < 2) {
    host.innerHTML = '<div class="chart-empty">no trades yet — chart appears after the first fill on this market</div>';
    return;
  }
  const px = candles.flatMap(c => [c.h, c.l]);
  const lo = Math.min(...px), hi = Math.max(...px);
  const range = (hi - lo) || 1;
  const lo2 = lo - range * 0.04, hi2 = hi + range * 0.04;
  const vMax = Math.max(...candles.map(c => c.v)) || 1;
  const cw = Math.max(2, Math.floor(innerW / candles.length) - 1);
  const x = (i) => padL + i * (innerW / candles.length) + (innerW / candles.length - cw) / 2;
  const y = (p) => padT + ((hi2 - p) / (hi2 - lo2)) * priceH;
  const yv = (v) => padT + priceH + 6 + (1 - v / vMax) * volH;
  const gridLines = [];
  for (let i = 0; i <= 4; i++) {
    const v = lo2 + (hi2 - lo2) * (1 - i / 4);
    const yy = padT + (i / 4) * priceH;
    gridLines.push(`<line x1="${padL}" y1="${yy}" x2="${W-padR}" y2="${yy}" stroke="#1a2230" stroke-width="1"/>`);
    gridLines.push(`<text x="${padL - 4}" y="${yy + 3}" text-anchor="end" fill="#8b949e" font-size="10" font-family="ui-monospace">${v.toLocaleString(undefined,{maximumFractionDigits:2})}</text>`);
  }
  const tMarks = [0, Math.floor(candles.length/2), candles.length-1];
  for (const i of tMarks) {
    const dt = new Date(candles[i].t * 1000).toISOString().slice(11,16);
    gridLines.push(`<text x="${x(i) + cw/2}" y="${H - 6}" text-anchor="middle" fill="#8b949e" font-size="10" font-family="ui-monospace">${dt}</text>`);
  }
  const candleSvg = candles.map((c, i) => {
    const up = c.c >= c.o;
    const col = up ? '#3fb950' : '#f85149';
    const yo = y(c.o), yc = y(c.c), yh = y(c.h), yl = y(c.l);
    const top = Math.min(yo, yc), bot = Math.max(yo, yc);
    const bh = Math.max(1, bot - top);
    const wickX = x(i) + cw/2;
    return `<g data-i="${i}"><line x1="${wickX}" y1="${yh}" x2="${wickX}" y2="${yl}" stroke="${col}" stroke-width="1"/><rect x="${x(i)}" y="${top}" width="${cw}" height="${bh}" fill="${col}" stroke="${col}"/></g>`;
  }).join('');
  const volSvg = candles.map((c, i) => {
    const up = c.c >= c.o; const col = up ? 'rgba(63,185,80,.6)' : 'rgba(248,81,73,.6)';
    const yy = yv(c.v); const hh = padT + priceH + 6 + volH - yy;
    return `<rect x="${x(i)}" y="${yy}" width="${cw}" height="${Math.max(1,hh)}" fill="${col}"/>`;
  }).join('');
  const last = candles[candles.length-1];
  const yLast = y(last.c);
  const lastCol = last.c >= last.o ? '#3fb950' : '#f85149';
  const lastLine = `<line x1="${padL}" y1="${yLast}" x2="${W-padR}" y2="${yLast}" stroke="${lastCol}" stroke-width="1" stroke-dasharray="3 3" opacity=".7"/>
    <rect x="${W-padR-58}" y="${yLast-8}" width="58" height="14" fill="${lastCol}"/>
    <text x="${W-padR-2}" y="${yLast+3}" text-anchor="end" fill="white" font-size="10" font-family="ui-monospace" font-weight="600">${last.c.toLocaleString()}</text>`;
  const overlay = `<rect id="kl_hot" x="${padL}" y="${padT}" width="${innerW}" height="${innerH}" fill="transparent" cursor="crosshair"/>
    <line id="kl_vline" x1="0" y1="${padT}" x2="0" y2="${padT+priceH}" stroke="#8b949e" stroke-width="1" stroke-dasharray="2 3" opacity="0" pointer-events="none"/>`;
  host.innerHTML = `<svg viewBox="0 0 ${W} ${H}" preserveAspectRatio="none">${gridLines.join('')}${volSvg}${candleSvg}${lastLine}${overlay}</svg>
    <div class="chart-tip" id="kl_tip" style="display:none"></div>`;
  const svg = host.querySelector('svg');
  const hot = host.querySelector('#kl_hot');
  const vline = host.querySelector('#kl_vline');
  const tip = host.querySelector('#kl_tip');
  hot.addEventListener('mousemove', (e) => {
    const rect = svg.getBoundingClientRect();
    const px2 = (e.clientX - rect.left) * (W / rect.width);
    const i = Math.max(0, Math.min(candles.length - 1, Math.floor((px2 - padL) / (innerW / candles.length))));
    const c = candles[i];
    const dt = new Date(c.t * 1000).toISOString().slice(0,16).replace('T',' ');
    vline.setAttribute('x1', px2); vline.setAttribute('x2', px2); vline.setAttribute('opacity', '.9');
    tip.style.display = 'block';
    tip.innerHTML = `<dl>
      <dt>time</dt><dd>${dt}</dd>
      <dt>open</dt><dd>${c.o.toLocaleString()}</dd>
      <dt>high</dt><dd style="color:#3fb950">${c.h.toLocaleString()}</dd>
      <dt>low</dt><dd style="color:#f85149">${c.l.toLocaleString()}</dd>
      <dt>close</dt><dd>${c.c.toLocaleString()}</dd>
      <dt>volume</dt><dd>${c.v.toLocaleString()}</dd>
      <dt>change</dt><dd style="color:${c.c>=c.o?'#3fb950':'#f85149'}">${c.o ? (((c.c-c.o)/c.o)*10000).toFixed(0) : '0'} bps</dd>
    </dl>`;
  });
  hot.addEventListener('mouseleave', () => { vline.setAttribute('opacity','0'); tip.style.display = 'none'; });
}

// Module-level state — only one active stream at a time per chart host.
const activeStreams = new Map(); // hostId -> { stream, market, candles, intervalSec }

// Stop the stream for a host (called from page navigation / market switch).
// Guarded for placeholder slots whose stream is still null during REST bootstrap.
export function stopKlineStream(hostId) {
  const cur = activeStreams.get(hostId);
  if (cur) { cur.stream?.close?.(); activeStreams.delete(hostId); }
}
export function stopAllKlineStreams() {
  for (const [, v] of activeStreams) v.stream?.close?.();
  activeStreams.clear();
}

// Bootstraps the chart with REST history, then opens a WebSocket subscription
// that mutates the candle array on each tick/fill and re-renders.
//
// Sources:
//   'local'   — backend's /markets/{m}/trades + /ws/trades/{m}; aggregates
//               raw trades into candles client-side via foldTradeIntoCandles.
//   'binance' — public Binance REST + WS (klines endpoint). Returns OHLCV
//               directly; the in-progress candle is updated in place per tick.
//
// Race-safe: every load is tagged with a Symbol token. If a newer load supersedes
// us during the REST await (rapid market switch / source flip), the in-flight
// request silently drops its results.
export async function loadKline(hostId, market, intervalSec, source = 'local') {
  const host = document.getElementById(hostId); if (!host) return;
  // Same-market/same-TF/same-source: just re-render the cached candles.
  const prev = activeStreams.get(hostId);
  if (prev && prev.market === market && prev.intervalSec === intervalSec && prev.source === source && Array.isArray(prev.candles)) {
    requestAnimationFrame(() => {
      const h = document.getElementById(hostId); if (h) renderKline(h, prev.candles);
    });
    return;
  }
  // Tear down any prior real stream synchronously, then claim the slot with a token.
  if (prev?.stream?.close) prev.stream.close();
  const token = Symbol('kline-load');
  activeStreams.set(hostId, { token, market, intervalSec, source, candles: null, stream: null, _loading:true });
  host.innerHTML = '<div class="chart-empty">loading…</div>';

  // ── REST bootstrap ──
  let candles;
  try {
    if (source === 'binance') {
      candles = await fetchBinanceKlines(market, intervalSec, 500);
    } else {
      const r = await get(`/markets/${encodeURIComponent(market)}/trades`, { silent:true });
      const slotChk = activeStreams.get(hostId);
      if (!slotChk || slotChk.token !== token) return;
      const trades = Array.isArray(r.json) ? r.json : (r.json?.trades || []);
      candles = aggregateTrades(trades, intervalSec);
    }
  } catch (e) {
    console.error('[charts] kline REST bootstrap failed', { hostId, market, intervalSec, source, error: e });
    const h = document.getElementById(hostId);
    if (h && activeStreams.get(hostId)?.token === token) {
      const detail = e?.message || String(e);
      h.innerHTML = `<div class="chart-empty">data load failed: ${escapeHtml(detail)}<br><small style="color:var(--mute)">source=${escapeHtml(source)} · check DevTools console for the [binance] REST log</small></div>`;
    }
    return;
  }
  // Bail if a newer load superseded us, or if the page has navigated away.
  const slot = activeStreams.get(hostId);
  if (!slot || slot.token !== token) return;
  if (!document.getElementById(hostId)) return;

  // ── repaint + status helpers ──
  let scheduled = false;
  const requestPaint = () => {
    if (scheduled) return;
    scheduled = true;
    requestAnimationFrame(() => {
      scheduled = false;
      if (activeStreams.get(hostId)?.token !== token) return;
      const h = document.getElementById(hostId);
      if (h) renderKline(h, candles);
    });
  };
  const statusMap = {
    connecting: ['connecting…', '#8b949e'],
    open:       ['live (ws)',   '#3fb950'],
    reconnect:  ['reconnect…',  '#d29922'],
    fallback:   ['polling',     '#d29922'],
    error:      ['ws error',    '#f85149'],
  };
  const onStatus = (s) => {
    const tag = document.getElementById(hostId + '_status');
    if (!tag) return;
    const baseLabel = s.kind === 'reconnect' && s.in_ms != null ? `reconnect in ${Math.round(s.in_ms/1000)}s` : statusMap[s.kind]?.[0] || s.kind;
    const col = statusMap[s.kind]?.[1] || '#8b949e';
    tag.style.color = col;
    tag.textContent = source === 'binance' && s.kind === 'open' ? 'live (binance)' : baseLabel;
  };

  // ── live subscription ──
  let stream;
  if (source === 'binance') {
    stream = new BinanceKlineStream(market, intervalSec, {
      onCandle: (c) => {
        if (activeStreams.get(hostId)?.token !== token) return;
        // Binance pushes a candle per WS tick; the final tick of a bucket has
        // closed=true. Update in place when bucket matches the tail; push when
        // a new bucket opens.
        const last = candles[candles.length - 1];
        if (last && last.t === c.t) {
          last.o = c.o; last.h = c.h; last.l = c.l; last.c = c.c; last.v = c.v;
        } else if (!last || c.t > last.t) {
          candles.push({ t:c.t, o:c.o, h:c.h, l:c.l, c:c.c, v:c.v });
          // Cap history so we don't grow unbounded over a long session.
          if (candles.length > 1000) candles.splice(0, candles.length - 1000);
        }
        requestPaint();
      },
      onStatus,
    });
  } else {
    stream = new TradeStream(market, {
      onTrade: (trade) => {
        if (activeStreams.get(hostId)?.token !== token) return;
        foldTradeIntoCandles(candles, trade, intervalSec);
        requestPaint();
      },
      onStatus,
    });
  }
  activeStreams.set(hostId, { token, stream, market, intervalSec, source, candles });
  stream.start();
  requestPaint();
}

// Note: router.js calls stopAllKlineStreams() explicitly before clearPageTimers()
// on every route change, so any open WS subscriptions are cleanly torn down.

// ─── Donut ─────────────────────────────────────────────────────────
export const DONUT_PALETTE = ['#58a6ff','#3fb950','#d29922','#f85149','#a371f7','#39c5cf','#db61a2','#ff7b72','#7ee787','#ffa657'];
export function renderDonut(host, slices, opts = {}) {
  const total = slices.reduce((s,x)=>s+x.value, 0);
  if (!slices.length || total <= 0) {
    host.innerHTML = '<div style="color:var(--mute);font-size:12px;padding:14px">no data</div>';
    return;
  }
  const cx = 100, cy = 100, rOuter = 88, rInner = 56;
  let acc = 0;
  const arcs = slices.map((s, i) => {
    const a0 = (acc / total) * Math.PI * 2 - Math.PI / 2;
    acc += s.value;
    const a1 = (acc / total) * Math.PI * 2 - Math.PI / 2;
    const large = (a1 - a0) > Math.PI ? 1 : 0;
    const x0 = cx + rOuter * Math.cos(a0), y0 = cy + rOuter * Math.sin(a0);
    const x1 = cx + rOuter * Math.cos(a1), y1 = cy + rOuter * Math.sin(a1);
    const x2 = cx + rInner * Math.cos(a1), y2 = cy + rInner * Math.sin(a1);
    const x3 = cx + rInner * Math.cos(a0), y3 = cy + rInner * Math.sin(a0);
    const col = s.color || DONUT_PALETTE[i % DONUT_PALETTE.length];
    return `<path d="M ${x0} ${y0} A ${rOuter} ${rOuter} 0 ${large} 1 ${x1} ${y1} L ${x2} ${y2} A ${rInner} ${rInner} 0 ${large} 0 ${x3} ${y3} Z" fill="${col}" stroke="#0d1117" stroke-width="1.5"><title>${escapeHtml(s.label)}: ${s.value.toLocaleString()} (${(s.value/total*100).toFixed(1)}%)</title></path>`;
  }).join('');
  const centerLabel = opts.centerLabel || 'total';
  const centerVal = opts.centerVal != null ? opts.centerVal : total.toLocaleString();
  const legend = `<div class="donut-legend"><table>${
    slices.map((s, i) => {
      const col = s.color || DONUT_PALETTE[i % DONUT_PALETTE.length];
      const pct = (s.value / total * 100).toFixed(1);
      return `<tr><td class="sw"><span style="background:${col}"></span></td><td><strong>${escapeHtml(s.label)}</strong></td><td class="num">${s.value.toLocaleString()}</td><td class="num" style="color:var(--mute)">${pct}%</td></tr>`;
    }).join('')
  }</table></div>`;
  host.innerHTML = `<div class="donut-wrap">
    <svg class="donut-svg" viewBox="0 0 200 200">
      ${arcs}
      <g class="donut-center" text-anchor="middle">
        <text class="lbl" x="100" y="92">${escapeHtml(centerLabel)}</text>
        <text class="val" x="100" y="115">${escapeHtml(String(centerVal))}</text>
      </g>
    </svg>
    ${legend}
  </div>`;
}
