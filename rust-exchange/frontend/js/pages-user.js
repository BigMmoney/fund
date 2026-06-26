// js/pages-user.js — User app page renderers.
import {
  $, escapeHtml, get, post, del_, call, asAdmin, uuidv4, hmacHex,
  form, input, select, textarea, table, stub, renderJSON, showResult,
  MARKETS, DEFAULT_MARKET, CHAIN_ASSETS, marketSelect, chainSelect, assetOptionsForChain, repaintAssetSelect, validateAddress,
  toast, confirmModal, notifyEvent, setPageInterval,
} from './core.js';
import { renderDonut, loadKline } from './charts.js';
import {
  fetchBinanceTicker, fetchBinanceDepth, fetchBinanceTrades,
  BinanceTickerStream, BinanceDepthStream, BinanceTradeStream,
  formatBinanceError,
} from './binance.js';

export function renderUserDashboard(c) {
  c.innerHTML = `
    <h2>Dashboard <small>account snapshot</small></h2>
    <div class="stats" id="dash_stats">
      <div class="stat"><div class="label">total balance</div><div class="val" id="ds_bal">…</div><div class="sub" id="ds_bal_sub">across all assets</div></div>
      <div class="stat"><div class="label">open orders</div><div class="val" id="ds_orders">…</div><div class="sub" id="ds_orders_sub">live in the book</div></div>
      <div class="stat"><div class="label">pending withdrawals</div><div class="val" id="ds_wd">…</div><div class="sub" id="ds_wd_sub">awaiting confirmation</div></div>
      <div class="stat ok" id="ds_status_card"><div class="label">api status</div><div class="val" id="ds_status">…</div><div class="sub" id="ds_status_sub"></div></div>
    </div>
    <div class="split">
      <div class="card"><h3>asset distribution</h3><div id="dash_donut">loading…</div></div>
      <div class="card">
        <h3>quick actions</h3>
        <div class="row btns">
          <button class="primary" onclick="location.hash='#user/deposit'">deposit</button>
          <button onclick="location.hash='#user/withdraw'">withdraw</button>
          <button onclick="location.hash='#user/trade'">trade</button>
          <button onclick="location.hash='#user/addresses'">address book</button>
        </div>
        <h3 style="margin-top:14px">recent orders</h3>
        <div id="dash_orders_table"></div>
      </div>
    </div>
    <div class="card"><h3>recent activity</h3><div id="dash_timeline">loading…</div></div>`;
  loadDashboard();
}
async function loadDashboard() {
  const subj = $('subject').value;
  const [h, bals, ords, wds] = await Promise.all([
    get('/health', { silent:true }),
    get('/balances/' + encodeURIComponent(subj), { silent:true }),
    get('/orders/' + encodeURIComponent(subj), { silent:true }),
    get('/v2/wallet/withdrawals', { silent:true }),
  ]);
  if (!$('ds_status')) return;
  if (h.json) {
    $('ds_status').textContent = h.json.status || '?';
    $('ds_status_sub').textContent = `up ${h.json.uptime_secs}s · ${h.json.accounts} accts`;
    $('ds_status_card').className = 'stat ' + (h.json.status==='ok' ? 'ok' : 'err');
  } else { $('ds_status').textContent = 'down'; $('ds_status_card').className='stat err'; }
  const bArr = Array.isArray(bals.json) ? bals.json : (bals.json?.balances || []);
  const totalAvail = bArr.reduce((s,b)=>s+(parseInt(b.available,10)||0), 0);
  const totalHold  = bArr.reduce((s,b)=>s+(parseInt(b.hold,10)||0), 0);
  $('ds_bal').textContent = totalAvail.toLocaleString();
  $('ds_bal_sub').textContent = `${bArr.length} asset(s)${totalHold ? ` · ${totalHold.toLocaleString()} on hold` : ''}`;
  const oArr = Array.isArray(ords.json) ? ords.json : (ords.json?.orders || []);
  const open = oArr.filter(o => !['filled','cancelled','canceled','rejected','expired'].includes(o.status));
  $('ds_orders').textContent = open.length;
  $('ds_orders_sub').textContent = `${oArr.length} total this session`;
  const wArr = Array.isArray(wds.json) ? wds.json : (wds.json?.withdrawals || []);
  const pending = wArr.filter(w => !['settled','rejected','removed'].includes(w.status));
  $('ds_wd').textContent = pending.length;
  $('ds_wd_sub').textContent = `${wArr.length} total · ${wArr.filter(w=>w.status==='settled').length} settled`;
  if ($('dash_donut')) {
    const slices = bArr.map(b => ({ label: b.asset, value: (parseInt(b.available,10)||0) + (parseInt(b.hold,10)||0) })).filter(s => s.value > 0).sort((a,b)=>b.value-a.value);
    if (slices.length === 0) $('dash_donut').innerHTML = '<em style="color:var(--mute)">no balances yet — go to Deposit</em>';
    else renderDonut($('dash_donut'), slices, { centerLabel:'aggregate', centerVal: (totalAvail+totalHold).toLocaleString() });
  }
  const events = [
    ...wArr.slice(0,6).map(w => ({ ts: w.submitted_at, type:'wd', label:`${w.status||'?'} · ${(w.chain||'').toUpperCase()} ${w.amount ?? '?'} → ${(w.destination_address||'').slice(0,18)}${(w.destination_address||'').length>18?'…':''}` })),
    ...oArr.slice(0,6).map(o => ({ ts: o.submitted_at || o.created_at, type:'trd', label:`${o.side||'?'} ${o.amount ?? '?'} @ ${o.price ?? '?'} ${o.market_id || o.market || ''} · ${o.status||'?'}` })),
  ].filter(e => e.ts).sort((a,b) => (b.ts||'').localeCompare(a.ts||''));
  $('dash_timeline').innerHTML = events.length === 0 ? '<em>no recent activity — try Deposit then Trade</em>' : events.slice(0,12).map(e => `
    <div class="timeline-row">
      <div class="icon ${e.type}">${e.type==='wd'?'↑':e.type==='dep'?'↓':'↔'}</div>
      <div class="ts">${(e.ts||'').slice(11,19)}</div>
      <div>${escapeHtml(e.label)}</div>
    </div>`).join('');
  $('dash_orders_table').innerHTML = oArr.length === 0 ? '<em>none</em>' : table(
    ['market','side','price','amount','status'],
    oArr.slice(0,5).map(o => [o.market_id || o.market, o.side, o.price, o.amount, `<span class="pill">${o.status||''}</span>`]));
}

export function renderUserMarkets(c) {
  c.innerHTML = `
    <h2>Markets <small>click a row to inspect</small></h2>
    <div class="split">
      <div class="card">
        <h3>all markets <button onclick="userMarketsLoad()" style="float:right;font-size:11px;padding:2px 8px">refresh</button></h3>
        <div id="markets_summary">loading…</div>
      </div>
      <div class="card">
        <h3 id="md_title">select a market</h3>
        <div id="market_detail"><em>click a row in the table</em></div>
      </div>
    </div>`;
  userMarketsLoad();
}
export async function userMarketsLoad() {
  const r = await get('/markets/summary', { silent:true });
  const seen = r.json?.markets || [];
  const seenIds = new Set(seen.map(m => m.market_id));
  const stub2 = MARKETS.filter(m => !seenIds.has(m)).map(m => ({ market_id:m, last_price:null, best_bid:null, best_ask:null, volume_24h:0, price_change_bps_24h:null, untraded:true }));
  const ms = [...seen, ...stub2];
  if (ms.length === 0) { $('markets_summary').innerHTML = '<em>no markets</em>'; return; }
  $('markets_summary').innerHTML = `<table><thead><tr><th>market</th><th>last</th><th>best bid</th><th>best ask</th><th>24h vol</th><th>24h Δ bps</th></tr></thead><tbody>${
    ms.map(m => `<tr style="cursor:pointer${m.untraded?';opacity:.55':''}" onclick="userMarketDetail('${m.market_id}')">
      <td><strong>${m.market_id}</strong>${m.untraded ? ' <span class="pill">untraded</span>' : ''}</td>
      <td class="num">${m.last_price ?? '—'}</td>
      <td class="num" style="color:var(--ok)">${m.best_bid ?? '—'}</td>
      <td class="num" style="color:var(--err)">${m.best_ask ?? '—'}</td>
      <td class="num">${m.volume_24h ?? 0}</td>
      <td class="num">${m.price_change_bps_24h ?? '—'}</td>
    </tr>`).join('')
  }</tbody></table>`;
}
export async function userMarketDetail(mkt) {
  $('md_title').textContent = mkt;
  $('market_detail').innerHTML = 'loading…';
  const [t, b, tr] = await Promise.all([
    get(`/markets/${encodeURIComponent(mkt)}/ticker`, { silent:true }),
    get(`/markets/${encodeURIComponent(mkt)}/book`, { silent:true }),
    get(`/markets/${encodeURIComponent(mkt)}/trades`, { silent:true }),
  ]);
  let html = '';
  if (t.json) html += `<dl class="kv"><dt>last</dt><dd>${t.json.last_price ?? '—'}</dd><dt>24h vol</dt><dd>${t.json.volume_24h ?? 0}</dd><dt>24h trades</dt><dd>${t.json.trade_count_24h ?? 0}</dd><dt>updated</dt><dd>${(t.json.timestamp||'').slice(11,19)}</dd></dl>`;
  html += `<div style="margin-top:10px;display:flex;align-items:center;gap:8px"><span style="font-size:11px;color:var(--mute)">stream:</span><span id="md_chart_status" style="font-size:11px">connecting…</span></div>`;
  html += `<div style="margin-top:6px"><div class="chart-host" id="md_chart" style="height:220px"></div></div>`;
  if (b.status === 200 && b.json) {
    const bids = b.json.bids || [], asks = b.json.asks || [];
    if (bids.length || asks.length) {
      const ascAsks = [...asks].slice(0,8).reverse();
      const bestBid = bids[0]?.price, bestAsk = asks[0]?.price;
      const spread = (bestAsk!=null && bestBid!=null) ? (bestAsk - bestBid) : null;
      html += '<h3>book</h3>';
      html += ascAsks.map(a => `<div class="ob-row ask"><div class="px">${a.price}</div><div class="qty">${a.quantity}</div></div>`).join('');
      html += `<div class="ob-spread">spread: ${spread ?? '—'}</div>`;
      html += bids.slice(0,8).map(b2 => `<div class="ob-row bid"><div class="px">${b2.price}</div><div class="qty">${b2.quantity}</div></div>`).join('');
    } else { html += '<h3>book</h3><em>empty</em>'; }
  } else { html += '<h3>book</h3><em>not loaded</em>'; }
  const trades = Array.isArray(tr.json) ? tr.json : (tr.json?.trades || []);
  if (trades.length) {
    html += '<h3>recent trades</h3>' + table(['time','price','qty','side'], trades.slice(0,8).map(x => [(x.timestamp||'').slice(11,19), x.price, x.quantity, x.side]));
  }
  $('market_detail').innerHTML = html;
  loadKline('md_chart', mkt, 60, window.__klineSource || 'binance');
}

window.__klineTf = 60;
window.__klineSource = localStorage.getItem('klineSource') || 'binance';
export function renderUserTrade(c) {
  const src = window.__klineSource || 'binance';
  c.innerHTML = `
    <h2>Trade <small>place orders against the matching engine</small></h2>
    <div class="row" style="margin-bottom:10px">
      <label style="width:80px">market</label>${marketSelect('o_market', DEFAULT_MARKET, 'userTradeRefresh()')}
      <button onclick="userTradeRefresh()">refresh book</button>
      <label style="width:80px;margin-left:14px">auto</label>
      <select id="o_auto"><option value="0">off</option><option value="2">2s</option><option value="5">5s</option></select>
      <small style="margin-left:auto;color:var(--mute)">supported pairs: ${MARKETS.join(' · ')}</small>
    </div>
    <div class="card chart-card">
      <div class="chart-toolbar">
        <strong id="kl_market_label">${DEFAULT_MARKET}</strong>
        <span id="kl_host_status" style="font-size:11px;color:var(--mute);margin-left:8px">connecting…</span>
        <span style="margin-left:14px;font-size:11px;color:var(--mute)">source:</span>
        <a class="tf ${src==='binance'?'active':''}" data-src="binance" id="kl_src_binance">Binance</a>
        <a class="tf ${src==='local'?'active':''}" data-src="local" id="kl_src_local">Local engine</a>
        <span style="margin-left:auto"></span>
        ${[['1m',60],['5m',300],['15m',900],['1h',3600]].map(([k,v]) =>
          `<a class="tf ${v===60?'active':''}" data-tf="${v}">${k}</a>`).join('')}
      </div>
      <div id="kl_ticker_strip" style="display:flex;gap:14px;flex-wrap:wrap;font-size:11px;color:var(--mute);padding:6px 12px;border-bottom:1px solid var(--border)">
        <span>last <span id="tkr_last">—</span></span>
        <span>24h Δ <span id="tkr_chg">—</span></span>
        <span>24h high <span id="tkr_hi">—</span></span>
        <span>24h low <span id="tkr_lo">—</span></span>
        <span>24h vol <span id="tkr_vol">—</span></span>
        <span style="margin-left:auto" id="tkr_status">—</span>
      </div>
      <div class="chart-host" id="kl_host"></div>
    </div>
    <div class="split">
      <div class="card">
        <h3>order book</h3>
        <div id="ob_view">loading…</div>
        <h3>recent trades</h3>
        <div id="trade_recent"></div>
      </div>
      <div>
        <div class="card">
          <h3>place order</h3>
          <div class="side-toggle" id="ot_side_tog">
            <a class="buy active" data-side="buy">BUY</a>
            <a class="sell" data-side="sell">SELL</a>
          </div>
          ${form([
            ['type', select('o_type',['limit','market'])],
            ['price (int)', input('o_price','50000')],
            ['amount (int)', input('o_qty','1')],
            ['outcome', input('o_outcome','0')],
            ['client_order_id', input('o_client','', 'auto')],
          ], `<button class="primary" id="ot_submit" onclick="userOrderSubmit()">place buy order</button>`)}
        </div>
        <div class="card">
          <h3>cancel order</h3>
          ${form([['order_id', input('o_cancel_id','')]], `<button class="danger" onclick="userOrderCancel()">cancel</button>`)}
        </div>
      </div>
    </div>`;
  document.querySelectorAll('#ot_side_tog a').forEach(a => a.addEventListener('click', () => {
    document.querySelectorAll('#ot_side_tog a').forEach(x => x.classList.remove('active'));
    a.classList.add('active');
    $('ot_submit').textContent = `place ${a.dataset.side} order`;
    $('ot_submit').className = a.dataset.side === 'buy' ? 'primary' : 'danger';
  }));
  $('o_auto').addEventListener('change', () => {
    if (window.__obTimer) { clearInterval(window.__obTimer); window.__obTimer = null; }
    const v = parseInt($('o_auto').value, 10);
    if (v > 0) {
      window.__obTimer = setPageInterval(() => {
        if (!document.getElementById('o_market')) return;
        userTradeRefresh();
      }, v * 1000);
    }
  });
  // TF picker
  document.querySelectorAll('.chart-toolbar a.tf[data-tf]').forEach(a => a.addEventListener('click', () => {
    document.querySelectorAll('.chart-toolbar a.tf[data-tf]').forEach(x => x.classList.remove('active'));
    a.classList.add('active');
    window.__klineTf = parseInt(a.dataset.tf, 10);
    loadKline('kl_host', $('o_market').value, window.__klineTf, window.__klineSource);
  }));
  // Source picker (Binance public feed vs local matching engine).
  // Source applies to every market-data stream on this page: candles,
  // ticker, order book, and trades.
  document.querySelectorAll('.chart-toolbar a.tf[data-src]').forEach(a => a.addEventListener('click', () => {
    document.querySelectorAll('.chart-toolbar a.tf[data-src]').forEach(x => x.classList.remove('active'));
    a.classList.add('active');
    window.__klineSource = a.dataset.src;
    localStorage.setItem('klineSource', window.__klineSource);
    loadKline('kl_host', $('o_market').value, window.__klineTf, window.__klineSource);
    void mountUserTradeStreams($('o_market').value, window.__klineSource);
  }));
  userTradeRefresh();
  loadKline('kl_host', $('o_market').value, window.__klineTf, window.__klineSource);
  void mountUserTradeStreams($('o_market').value, window.__klineSource || 'binance');
}

// ─── Trade-page Binance / local market-data plumbing ─────────────
//
// Binance: subscribe to ticker / depth / trades streams via BinanceProvider
// (frontend/js/binance.js). Each stream auto-reconnects on its own.
//
// Local: keep the existing REST-based refresh in userTradeRefresh (unchanged
// for non-Binance source). The ticker strip is hidden because the local
// engine has no equivalent ticker WS.
//
// Mount/unmount are tied to the page's lifecycle: router.js calls
// stopUserTradeStreams() on page change.

const _userTradeStreams = { ticker: null, depth: null, trade: null, market: null, source: null };

function _renderTickerSnap(t) {
  const el = (id) => document.getElementById(id);
  if (!el('tkr_last')) return; // nav happened
  el('tkr_last').textContent = t?.last_price != null ? t.last_price.toLocaleString() : '—';
  const chg = t?.price_change ?? 0;
  el('tkr_chg').textContent = t?.price_change != null
    ? `${chg.toLocaleString(undefined, { maximumFractionDigits: 2 })} (${(t.price_change_percent ?? 0).toFixed(2)}%)`
    : '—';
  el('tkr_chg').style.color = chg >= 0 ? '#3fb950' : '#f85149';
  el('tkr_hi').textContent  = t?.high_price != null ? t.high_price.toLocaleString() : '—';
  el('tkr_lo').textContent  = t?.low_price  != null ? t.low_price.toLocaleString()  : '—';
  el('tkr_vol').textContent = t?.volume     != null ? t.volume.toLocaleString(undefined, { maximumFractionDigits: 2 }) : '—';
}
function _setTickerStatus(text, color) {
  const el = document.getElementById('tkr_status');
  if (!el) return;
  el.textContent = text;
  el.style.color = color || 'var(--mute)';
}
function _renderBookFromBinance(book) {
  const ob = document.getElementById('ob_view');
  if (!ob) return;
  const bids = book?.bids || [];
  const asks = book?.asks || [];
  if (!bids.length && !asks.length) { ob.innerHTML = '<em>book empty</em>'; return; }
  const ascAsks = asks.slice(0, 12).reverse();
  const spread = (asks[0]?.price != null && bids[0]?.price != null) ? (asks[0].price - bids[0].price) : null;
  const fmtP = (v) => v?.toLocaleString(undefined, { maximumFractionDigits: 2 });
  const fmtQ = (v) => v?.toLocaleString(undefined, { maximumFractionDigits: 6 });
  ob.innerHTML =
    ascAsks.map(a => `<div class="ob-row ask" onclick="document.getElementById('o_price').value='${a.price}'" style="cursor:pointer"><div class="px">${fmtP(a.price)}</div><div class="qty">${fmtQ(a.quantity)}</div></div>`).join('') +
    `<div class="ob-spread">spread: ${spread != null ? fmtP(spread) : '—'} · best bid ${fmtP(bids[0]?.price) ?? '—'} · best ask ${fmtP(asks[0]?.price) ?? '—'}</div>` +
    bids.slice(0, 12).map(b => `<div class="ob-row bid" onclick="document.getElementById('o_price').value='${b.price}'" style="cursor:pointer"><div class="px">${fmtP(b.price)}</div><div class="qty">${fmtQ(b.quantity)}</div></div>`).join('');
}
const _liveTrades = []; // ring of last N
function _addLiveTrade(t) {
  _liveTrades.unshift(t);
  if (_liveTrades.length > 30) _liveTrades.length = 30;
  _renderLiveTrades();
}
function _renderLiveTrades() {
  const el = document.getElementById('trade_recent');
  if (!el) return;
  if (!_liveTrades.length) { el.innerHTML = '<em>no trades yet</em>'; return; }
  el.innerHTML = table(['time','px','qty','side'],
    _liveTrades.slice(0, 8).map(x => [(x.timestamp || '').slice(11, 19), x.price, x.quantity, x.side]));
}

export async function mountUserTradeStreams(market, source) {
  // Tear down whatever was running, then start fresh based on source.
  stopUserTradeStreams();
  _userTradeStreams.market = market;
  _userTradeStreams.source = source;

  if (source !== 'binance') {
    // Local source: hide ticker strip (no equivalent WS) + let
    // userTradeRefresh keep the order book / trades panels populated.
    const strip = document.getElementById('kl_ticker_strip');
    if (strip) strip.style.display = 'none';
    return;
  }

  const strip = document.getElementById('kl_ticker_strip');
  if (strip) strip.style.display = '';

  // REST snapshot first so panels paint quickly; WS subscribe right after.
  _setTickerStatus('connecting…');
  try {
    const [t, d, tr] = await Promise.all([
      fetchBinanceTicker(market),
      fetchBinanceDepth(market, 20),
      fetchBinanceTrades(market, 30),
    ]);
    if (_userTradeStreams.market !== market || _userTradeStreams.source !== source) return;
    _renderTickerSnap(t);
    _renderBookFromBinance(d);
    _liveTrades.length = 0;
    for (const trade of tr) _liveTrades.push(trade);
    _renderLiveTrades();
  } catch (e) {
    _setTickerStatus(`snapshot: ${formatBinanceError(e)}`, '#f85149');
    // Don't return — WS may still connect and recover.
  }
  if (_userTradeStreams.market !== market || _userTradeStreams.source !== source) return;

  const onStatus = (kind) => (s) => {
    if (_userTradeStreams.market !== market || _userTradeStreams.source !== source) return;
    const map = { connecting: ['connecting…', '#8b949e'], open: ['live (binance)', '#3fb950'], reconnect: [`reconnect…`, '#d29922'], error: ['ws error', '#f85149'] };
    const [label, col] = map[s.kind] || [s.kind, '#8b949e'];
    if (kind === 'ticker') _setTickerStatus(label, col);
  };

  _userTradeStreams.ticker = new BinanceTickerStream(market, {
    onTicker: (t) => {
      if (_userTradeStreams.market !== market || _userTradeStreams.source !== source) return;
      _renderTickerSnap(t);
    },
    onStatus: onStatus('ticker'),
  });
  _userTradeStreams.depth = new BinanceDepthStream(market, {
    onDepth: (d) => {
      if (_userTradeStreams.market !== market || _userTradeStreams.source !== source) return;
      _renderBookFromBinance(d);
    },
    onStatus: () => {},
    levels: 20,
    speedMs: 100,
  });
  _userTradeStreams.trade = new BinanceTradeStream(market, {
    onTrade: (t) => {
      if (_userTradeStreams.market !== market || _userTradeStreams.source !== source) return;
      _addLiveTrade(t);
    },
    onStatus: () => {},
  });
  _userTradeStreams.ticker.start();
  _userTradeStreams.depth.start();
  _userTradeStreams.trade.start();
}

export function stopUserTradeStreams() {
  for (const k of ['ticker', 'depth', 'trade']) {
    try { _userTradeStreams[k]?.close?.(); } catch {}
    _userTradeStreams[k] = null;
  }
  _userTradeStreams.market = null;
  _userTradeStreams.source = null;
  _liveTrades.length = 0;
}
export async function userTradeRefresh() {
  const mEl = $('o_market'); if (!mEl) return;
  const m = mEl.value;
  const lbl = $('kl_market_label'); if (lbl) lbl.textContent = m;
  const source = window.__klineSource || 'binance';
  // Chart self-updates via WS — repaint only on market/source/tf change.
  loadKline('kl_host', m, window.__klineTf || 60, source);

  if (source === 'binance') {
    // Order book + trades + ticker are owned by mountUserTradeStreams.
    // Re-mount only when the market actually changed (avoids tearing down
    // WS connections on every UI poke).
    if (_userTradeStreams.market !== m || _userTradeStreams.source !== source) {
      void mountUserTradeStreams(m, source);
    }
    return;
  }

  // Local-engine source: existing REST poll path.
  if (_userTradeStreams.source !== 'local') stopUserTradeStreams();
  _userTradeStreams.market = m;
  _userTradeStreams.source = 'local';
  const [b, tr] = await Promise.all([
    get(`/markets/${encodeURIComponent(m)}/book`, { silent:true }),
    get(`/markets/${encodeURIComponent(m)}/trades`, { silent:true }),
  ]);
  if (!$('ob_view')) return;
  if (b.status === 200 && (b.json?.bids?.length || b.json?.asks?.length)) {
    const bids = b.json.bids || [], asks = b.json.asks || [];
    const ascAsks = [...asks].slice(0,12).reverse();
    const spread = (asks[0]?.price != null && bids[0]?.price != null) ? (asks[0].price - bids[0].price) : null;
    $('ob_view').innerHTML =
      ascAsks.map(a => `<div class="ob-row ask" onclick="document.getElementById('o_price').value='${a.price}'" style="cursor:pointer"><div class="px">${a.price}</div><div class="qty">${a.quantity}</div></div>`).join('') +
      `<div class="ob-spread">spread: ${spread ?? '—'} · best bid ${bids[0]?.price ?? '—'} · best ask ${asks[0]?.price ?? '—'}</div>` +
      bids.slice(0,12).map(b2 => `<div class="ob-row bid" onclick="document.getElementById('o_price').value='${b2.price}'" style="cursor:pointer"><div class="px">${b2.price}</div><div class="qty">${b2.quantity}</div></div>`).join('');
  } else { $('ob_view').innerHTML = '<em>book is empty — be the first to place an order</em>'; }
  const trades = Array.isArray(tr.json) ? tr.json : (tr.json?.trades || []);
  $('trade_recent').innerHTML = trades.length === 0 ? '<em>no trades yet</em>' : table(['time','px','qty','side'],
    trades.slice(0,8).map(x => [(x.timestamp||'').slice(11,19), x.price, x.quantity, x.side]));
}
export async function userOrderSubmit() {
  const sideEl = document.querySelector('#ot_side_tog a.active');
  const side = sideEl ? sideEl.dataset.side : 'buy';
  const price = parseInt($('o_price').value, 10);
  const amount = parseInt($('o_qty').value, 10);
  const outcome = parseInt($('o_outcome').value, 10);
  if (!Number.isFinite(price)  || price <= 0)   { toast('Price must be a positive integer', 'warn'); return; }
  if (!Number.isFinite(amount) || amount <= 0)  { toast('Amount must be a positive integer', 'warn'); return; }
  if (!Number.isFinite(outcome))                { toast('Outcome required (use 0 for spot)', 'warn'); return; }
  const market_id = ($('o_market').value || '').trim();
  if (!market_id) { toast('Market required', 'warn'); return; }
  const body = { market_id, side, order_type: $('o_type').value, price, amount, outcome, client_order_id: $('o_client').value || ('ui-' + uuidv4()) };
  const r = await post('/submit-order', body);
  if (r.status === 200) {
    toast(`${side.toUpperCase()} order placed`, 'ok', `${body.amount} @ ${body.price} ${body.market_id}`);
    notifyEvent('order_filled','order submitted',`${side} ${body.amount} @ ${body.price} ${body.market_id}`);
  } else {
    toast('Order rejected', 'err', `HTTP ${r.status}`);
  }
  setTimeout(userTradeRefresh, 200);
}
export async function userOrderCancel() {
  const body = { order_id: $('o_cancel_id').value, market_id: $('o_market').value };
  if (!body.order_id) { toast('order_id required', 'warn'); return; }
  const ok = await confirmModal({ title:'Cancel order?', body:`Cancel order <code>${escapeHtml(body.order_id.slice(0,18))}</code> on <code>${escapeHtml(body.market_id)}</code>?`, okLabel:'Cancel order', danger:true });
  if (!ok) return;
  const r = await post('/cancel-order', body);
  toast(r.status === 200 ? 'Order cancelled' : 'Cancel failed', r.status === 200 ? 'ok' : 'err');
  setTimeout(userTradeRefresh, 200);
}

window.__ordersFilter = 'open';
export function renderUserOrders(c) {
  c.innerHTML = `
    <h2>Orders <small>your orders across markets</small></h2>
    <div class="card">
      <div class="filter-pills" id="ord_pills">
        <a data-f="open" class="active">open</a><a data-f="filled">filled</a><a data-f="cancelled">cancelled</a><a data-f="all">all</a>
      </div>
      <div class="row btns">
        <button class="primary" onclick="userOrdersLoad()">refresh</button>
        <small id="ord_count" style="margin-left:auto;color:var(--mute)"></small>
      </div>
      <div id="orders_table"></div>
    </div>`;
  document.querySelectorAll('#ord_pills a').forEach(a => a.addEventListener('click', () => {
    document.querySelectorAll('#ord_pills a').forEach(x => x.classList.remove('active'));
    a.classList.add('active');
    window.__ordersFilter = a.dataset.f;
    userOrdersLoad();
  }));
  userOrdersLoad();
}
export async function userOrdersLoad() {
  const subj = $('subject').value;
  const r = await get('/orders/' + encodeURIComponent(subj));
  const orders = Array.isArray(r.json) ? r.json : (r.json?.orders || []);
  const f = window.__ordersFilter || 'open';
  const filt = orders.filter(o => {
    const st = (o.status||'').toLowerCase();
    if (f === 'all') return true;
    if (f === 'filled') return st === 'filled';
    if (f === 'cancelled') return ['cancelled','canceled','expired','rejected'].includes(st);
    return !['filled','cancelled','canceled','expired','rejected'].includes(st);
  });
  $('ord_count').textContent = `${filt.length} of ${orders.length} matching "${f}"`;
  if (filt.length === 0) { $('orders_table').innerHTML = '<em>no orders matching filter</em>'; return; }
  $('orders_table').innerHTML = `<table><thead><tr><th>id</th><th>market</th><th>side</th><th>type</th><th class="num">price</th><th class="num">amount</th><th>status</th><th></th></tr></thead><tbody>${
    filt.map(o => {
      const id = o.order_id;
      const isOpen = !['filled','cancelled','canceled','expired','rejected'].includes((o.status||'').toLowerCase());
      return `<tr>
        <td>${id ? id.slice(0,16) : ''}</td>
        <td>${o.market_id || o.market || ''}</td>
        <td><span class="pill ${o.side==='buy'?'ok':'err'}">${o.side}</span></td>
        <td>${o.order_type || ''}</td>
        <td class="num">${o.price ?? ''}</td>
        <td class="num">${o.amount ?? o.quantity ?? ''}</td>
        <td><span class="pill">${o.status||''}</span></td>
        <td>${isOpen ? `<button class="danger" onclick="userOrderCancelInline('${id}','${o.market_id || o.market}')">cancel</button>` : ''}</td>
      </tr>`;
    }).join('')
  }</tbody></table>`;
}
export async function userOrderCancelInline(id, mkt) {
  const ok = await confirmModal({ title:'Cancel order?', body:`Cancel order <code>${escapeHtml(id.slice(0,18))}</code> on <code>${escapeHtml(mkt)}</code>?`, okLabel:'Cancel order', danger:true });
  if (!ok) return;
  const r = await post('/cancel-order', { order_id: id, market_id: mkt });
  toast(r.status === 200 ? 'Order cancelled' : 'Cancel failed', r.status === 200 ? 'ok' : 'err');
  setTimeout(userOrdersLoad, 200);
}

export function renderUserBalances(c) {
  c.innerHTML = `
    <h2>Balances <small>per-asset breakdown</small></h2>
    <div class="row btns" style="margin-bottom:10px">
      <button class="primary" onclick="userBalLoad()">refresh</button>
      <small style="margin-left:auto;color:var(--mute)" id="bal_total"></small>
    </div>
    <div class="card"><h3>distribution</h3><div id="bal_donut">loading…</div></div>
    <div id="bal_grid">loading…</div>`;
  userBalLoad();
}
export async function userBalLoad() {
  const subj = $('subject').value;
  const r = await get('/balances/' + encodeURIComponent(subj));
  const arr = Array.isArray(r.json) ? r.json : (r.json?.balances || []);
  const total = arr.reduce((s,b)=>s + (parseInt(b.available,10)||0) + (parseInt(b.hold,10)||0), 0);
  $('bal_total').textContent = total ? `aggregate (raw): ${total.toLocaleString()}` : '';
  if (arr.length === 0) {
    $('bal_donut').innerHTML = '<em style="color:var(--mute)">no data</em>';
    $('bal_grid').innerHTML = '<em>no balances yet — go to Deposit page</em>';
    return;
  }
  const slices = arr.map(b => ({ label: b.asset, value: (parseInt(b.available,10)||0) + (parseInt(b.hold,10)||0) })).filter(s => s.value > 0).sort((a,b)=>b.value-a.value);
  renderDonut($('bal_donut'), slices, { centerLabel:'aggregate', centerVal: total.toLocaleString() });
  $('bal_grid').innerHTML = `<div class="stats">${
    arr.map(b => {
      const av = parseInt(b.available,10)||0, hd = parseInt(b.hold,10)||0;
      const tot = av + hd; const avPct = tot ? Math.round(av/tot*100) : 100;
      return `<div class="stat">
        <div class="label">${b.asset}</div>
        <div class="val">${av.toLocaleString()}</div>
        <div class="sub">on hold: ${hd.toLocaleString()} (${100-avPct}%)</div>
        <div class="meter" style="margin-top:8px"><span style="width:${avPct}%"></span><span class="warn" style="width:${100-avPct}%"></span></div>
        <div class="sub" style="margin-top:6px">updated ${(b.updated_at||'').slice(11,19)}</div>
      </div>`;
    }).join('')
  }</div>`;
}

export function renderUserDeposit(c) {
  c.innerHTML = `
    <h2>Deposit <small>fund your account · BTC · ETH · ERC-20 (USDT/USDC)</small></h2>
    <div class="split">
      <div class="card">
        <h3>production (on-chain)</h3>
        <div class="desc">Pick a chain + asset. The hot-wallet worker observes the address, waits for confirmations, and credits your balance once finalized. ETH and ERC-20 tokens (USDT, USDC) share the same per-user ETH deposit address — the asset is identified by the token contract.</div>
        <div class="row">
          <label style="width:80px">chain</label>${chainSelect('dep_chain','eth','depRepaintAsset()')}
          <label style="width:80px">asset</label><select id="dep_asset">${assetOptionsForChain('eth')}</select>
        </div>
        <div id="dep_addr_panel" class="summary-card"></div>
      </div>
      <div class="card">
        <h3>test deposit <span class="pill warn">testing only</span></h3>
        <div class="desc">Hits admin <code>/deposit</code> to seed your balance directly. Production users won't see this page.</div>
        ${form([
          ['user_id', input('dep_user','user-test-1')],
          ['amount',  input('dep_amount','100000000')],
          ['asset',   `<select id="dep_test_asset"><option>USDT</option><option>USDC</option><option>ETH</option><option>BTC</option></select>`],
          ['op_id',   input('dep_op','', 'auto-generated')],
        ], `<button class="primary" onclick="userDeposit()">credit balance</button>`)}
        <div id="dep_result"></div>
      </div>
    </div>`;
  depRepaintAsset();
}
export function depRepaintAsset() {
  repaintAssetSelect('dep_chain','dep_asset');
  const ch = $('dep_chain').value;
  const sym = $('dep_asset').value;
  const meta = (CHAIN_ASSETS[ch] || []).find(a => a.symbol === sym) || {};
  const conf = { eth: 25, btc: 6, sol: 32 }[ch] || '—';
  const finalisation = { eth: '≈ 6 min mainnet', btc: '≈ 60 min mainnet', sol: '≈ 30 s mainnet' }[ch] || '—';
  const subj = $('subject').value || 'user-test-1';
  const stubAddr = ch === 'btc'
    ? 'bc1q' + subj.replace(/[^a-z0-9]/g,'').padEnd(38,'0').slice(0,38)
    : ch === 'sol'
    ? (subj.replace(/[^A-Za-z0-9]/g,'') + '111111111111111111111111111111111').slice(0,44)
    : '0x' + (subj + '0000000000000000000000000000000000000000').replace(/[^a-zA-Z0-9]/g,'').slice(0,40).padEnd(40,'0');
  $('dep_addr_panel').innerHTML = `<dl>
    <div><dt>${ch.toUpperCase()} deposit address</dt><dd style="font-family:ui-monospace,Menlo,Consolas,monospace;word-break:break-all">${stubAddr} <span class="pill warn">stub</span></dd></div>
    ${meta.standard === 'erc20' ? `<div><dt>token contract</dt><dd style="font-family:ui-monospace,Menlo,Consolas,monospace;word-break:break-all">${meta.contract}</dd></div>
    <div><dt>standard</dt><dd>ERC-20 — send via the ETH address above; do NOT send from another chain.</dd></div>` : ''}
    ${meta.standard === 'spl' ? `<div><dt>token mint</dt><dd style="font-family:ui-monospace,Menlo,Consolas,monospace;word-break:break-all">${meta.contract}</dd></div>
    <div><dt>standard</dt><dd>SPL — send via the SOL address above; do NOT send from another chain.</dd></div>` : ''}
    <div><dt>required confirmations</dt><dd>${conf}</dd></div>
    <div><dt>credited at</dt><dd>finalization (${finalisation})</dd></div>
    <div><dt>asset</dt><dd>${sym}${meta.label ? ` — ${meta.label}` : ''} (${meta.decimals ?? '?'} decimals)</dd></div>
  </dl>`;
}
export async function userDeposit() {
  await asAdmin(async () => {
    const asset = $('dep_test_asset')?.value || 'USDT';
    const amount = parseInt($('dep_amount').value, 10);
    const body = { user_id: $('dep_user').value, amount, op_id: $('dep_op').value || ('ui-dep-' + uuidv4()), asset };
    const r = await post('/deposit', body);
    if (r.status === 200) {
      $('dep_result').innerHTML = `<div class="summary-card" style="border-left-color:var(--ok)"><strong>✓ credited</strong> ${amount.toLocaleString()} ${asset} to ${$('dep_user').value}</div>`;
      toast(`Credited ${amount.toLocaleString()} ${asset}`, 'ok', `to ${body.user_id}`);
      notifyEvent('order_filled','deposit credited',`${amount.toLocaleString()} ${asset}`);
    } else {
      $('dep_result').innerHTML = `<div class="summary-card" style="border-left-color:var(--err)"><strong>✗ failed</strong> ${r.status}: ${escapeHtml(r.text||'')}</div>`;
      toast('Deposit failed', 'err', `HTTP ${r.status}`);
    }
  })();
}

export function renderUserWithdraw(c) {
  c.innerHTML = `
    <h2>Withdraw <small>send to a whitelisted address · BTC · ETH · USDT/USDC (ERC-20)</small></h2>
    <div class="split">
      <div class="card">
        <h3>1. select address</h3>
        <div id="w_addr_list">loading…</div>
        <h3 style="margin-top:14px">2. asset + amount</h3>
        <div class="row">
          <label>asset</label><select id="w_asset"><option>—</option></select>
          <label>amount (smallest unit)</label>${input('w_amount','1000')}
        </div>
        <div class="row"><label>client_reference</label>${input('w_ref','','auto-generated if blank')}</div>
        <h3 style="margin-top:14px">3. preview & submit</h3>
        <div id="w_preview"><em>pick an address above to preview</em></div>
        <div class="row btns"><button class="primary" id="w_submit" disabled onclick="userSubmitWithdraw()">submit withdrawal</button></div>
      </div>
      <div class="card">
        <h3>recent withdrawals <button onclick="userListWds()" style="float:right;font-size:11px;padding:2px 8px">refresh</button></h3>
        <div id="wd_table"></div>
      </div>
    </div>`;
  loadWithdrawAddrs();
  userListWds();
  ['w_amount'].forEach(id => { const elx = $(id); if (elx) elx.addEventListener('input', updateWithdrawPreview); });
  $('w_asset')?.addEventListener('change', updateWithdrawPreview);
}
window.__withdrawAddr = null;
async function loadWithdrawAddrs() {
  const r = await get('/v2/wallet/addresses', { silent:true });
  const arr = Array.isArray(r.json) ? r.json : (r.json?.addresses || []);
  const eligible = arr.filter(a => ['active','approved','pending_cooldown'].includes(a.status));
  if (eligible.length === 0) { $('w_addr_list').innerHTML = '<em>no eligible addresses — add one in Address Book first</em>'; return; }
  $('w_addr_list').innerHTML = eligible.map(a => `
    <label style="display:block;padding:6px;border:1px solid var(--border);border-radius:4px;margin-bottom:4px;cursor:pointer">
      <input type="radio" name="w_addr" value="${a.address_id}" data-chain="${a.chain}" data-addr="${a.address}" />
      <span class="pill">${a.chain.toUpperCase()}</span>
      <strong style="margin-left:6px">${a.label || '(unnamed)'}</strong>
      <span style="color:var(--mute);font-size:11px;margin-left:6px">${a.address.slice(0,28)}…</span>
      <span class="pill" style="float:right">${a.status}</span>
    </label>`).join('');
  document.querySelectorAll('input[name=w_addr]').forEach(r2 => r2.addEventListener('change', () => {
    window.__withdrawAddr = { id: r2.value, chain: r2.dataset.chain, addr: r2.dataset.addr };
    const sel = $('w_asset'); if (sel) sel.innerHTML = assetOptionsForChain(r2.dataset.chain);
    updateWithdrawPreview();
  }));
}
function updateWithdrawPreview() {
  const a = window.__withdrawAddr;
  if (!a) { $('w_preview').innerHTML = '<em>pick an address above</em>'; $('w_submit').disabled = true; return; }
  const amt = parseInt($('w_amount').value, 10) || 0;
  const asset = $('w_asset')?.value || (CHAIN_ASSETS[a.chain]?.[0]?.symbol) || a.chain.toUpperCase();
  const meta = (CHAIN_ASSETS[a.chain] || []).find(x => x.symbol === asset) || {};
  const feeMap = { eth_native: 1_000_000, eth_erc20: 3_000_000, btc: 1_000, sol: 5_000 };
  const feeKey = a.chain === 'eth' ? (meta.standard === 'erc20' ? 'eth_erc20' : 'eth_native') : a.chain;
  const fee = feeMap[feeKey] || 0;
  const totalDebited = (meta.standard === 'erc20' || meta.standard === 'spl') ? amt : (amt + fee);
  $('w_preview').innerHTML = `
    <div class="summary-card"><dl>
      <div><dt>chain</dt><dd>${a.chain.toUpperCase()}</dd></div>
      <div><dt>asset</dt><dd>${asset}${meta.label ? ` <span style="color:var(--mute)">— ${meta.label}</span>` : ''}</dd></div>
      ${meta.standard === 'erc20' ? `<div><dt>token contract</dt><dd style="font-family:ui-monospace,Menlo,Consolas,monospace;font-size:11px">${meta.contract}</dd></div>` : ''}
      <div><dt>destination</dt><dd>${a.addr}</dd></div>
      <div><dt>amount</dt><dd>${amt.toLocaleString()} <span style="color:var(--mute)">${asset} (smallest unit)</span></dd></div>
      <div><dt>estimated fee</dt><dd>${fee.toLocaleString()} <span style="color:var(--mute)">${a.chain === 'eth' ? 'wei (paid in ETH)' : a.chain === 'sol' ? 'lamports (paid in SOL)' : 'satoshis'}</span></dd></div>
      <div><dt>total ${asset} debited</dt><dd><strong>${totalDebited.toLocaleString()}</strong></dd></div>
    </dl></div>`;
  $('w_submit').disabled = !(a && amt > 0 && asset && asset !== '—');
}
export async function userSubmitWithdraw() {
  const a = window.__withdrawAddr;
  if (!a) { toast('Pick an address first', 'warn'); return; }
  const amount = parseInt($('w_amount').value, 10);
  const asset = $('w_asset')?.value;
  if (!amount || amount <= 0) { toast('Amount must be > 0', 'warn'); return; }
  if (!asset || asset === '—') { toast('Pick an asset', 'warn'); return; }
  const ok = await confirmModal({
    title:'Submit withdrawal?',
    body:`Withdraw <strong>${amount.toLocaleString()} ${escapeHtml(asset)}</strong> to <code>${escapeHtml(a.addr)}</code> on <strong>${escapeHtml(a.chain.toUpperCase())}</strong>.<br/><br/>This is irreversible once on-chain confirmation lands.`,
    okLabel:'Submit', okClass:'primary',
  });
  if (!ok) return;
  const ref = $('w_ref').value || ('ui-wd-' + uuidv4());
  $('w_ref').value = ref;
  const meta = (CHAIN_ASSETS[a.chain] || []).find(x => x.symbol === asset) || {};
  const body = { chain: a.chain, destination_address: a.addr, amount, client_reference: ref, asset, token_standard: meta.standard, token_contract: meta.contract || undefined };
  const r = await post('/v2/wallet/withdraw', body);
  if (r.status === 200) {
    $('w_preview').innerHTML += `<div class="summary-card" style="border-left-color:var(--ok)"><strong>✓ submitted</strong><br/>id: ${r.json?.withdrawal_id}<br/>asset: ${asset}<br/>status: <span class="pill ok">${r.json?.status}</span></div>`;
    toast('Withdrawal submitted', 'ok', `${amount.toLocaleString()} ${asset} → ${a.addr.slice(0,18)}…`);
    notifyEvent(r.json?.status === 'awaiting_approval' ? 'withdrawal_awaiting' : 'withdrawal_settled', 'withdrawal '+(r.json?.status||'submitted'), `${amount.toLocaleString()} ${asset} on ${a.chain}`);
  } else {
    toast('Withdrawal rejected', 'err', `HTTP ${r.status}`);
    notifyEvent('withdrawal_rejected','withdrawal rejected',`HTTP ${r.status}`);
  }
  userListWds();
}
export async function userListWds() {
  const r = await get('/v2/wallet/withdrawals', { silent:true });
  const arr = Array.isArray(r.json) ? r.json : (r.json?.withdrawals || []);
  showResult('wd_table', arr.length === 0 ? '<em>none</em>' : table(
    ['id','status','chain','asset','amount','dest','tx_hash'],
    arr.slice(0,10).map(w => [
      w.withdrawal_id.slice(0,18),
      `<span class="pill">${w.status}</span>`,
      w.chain,
      w.asset || (CHAIN_ASSETS[w.chain]?.[0]?.symbol ?? '—'),
      w.amount,
      (w.destination_address||'').slice(0,16) + '…',
      (w.tx_hash||'').slice(0,16),
    ])));
}

export function renderUserAddresses(c) {
  c.innerHTML = `
    <h2>Address Book <small>whitelist withdrawal destinations</small></h2>
    <div class="card">
      <h3>add address</h3>
      <div class="row">
        <label>chain</label>${chainSelect('ab_chain','eth','userAddrValidate()')}
        <label>address</label>${input('ab_addr','')}
        <label>label</label>${input('ab_label','my-wallet')}
        <button class="primary" onclick="userAddAddr()">add</button>
      </div>
      <div id="ab_validate" class="vfeedback" style="margin-left:140px"></div>
      <small style="color:var(--mute)">
        New addresses go through sanctions screening + cool-down before they're eligible to withdraw to.
        <strong>ETH addresses serve ETH, USDT-ERC20, and USDC-ERC20</strong> — one whitelist entry covers all three. SOL addresses cover SOL + SPL USDC; BTC addresses cover BTC.
      </small>
    </div>
    <h3 style="margin-top:14px">my addresses <button onclick="userListAddrs()" style="float:right;font-size:11px;padding:2px 8px">refresh</button></h3>
    <div id="addr_grid">loading…</div>`;
  userListAddrs();
  const recheck = () => userAddrValidate();
  $('ab_addr').addEventListener('input', recheck);
  recheck();
}
export function userAddrValidate() {
  const chain = $('ab_chain')?.value || 'eth';
  const addr = $('ab_addr')?.value || '';
  const out = $('ab_validate'); if (!out) return;
  if (!addr) { out.textContent = ''; return; }
  const v = validateAddress(chain, addr);
  out.className = 'vfeedback ' + (v.ok ? 'ok' : 'err');
  out.textContent = (v.ok ? '✓ ' : '✗ ') + v.message;
}
export async function userAddAddr() {
  const chain = $('ab_chain').value;
  const address = $('ab_addr').value.trim();
  const label = $('ab_label').value.trim();
  const v = validateAddress(chain, address);
  if (!v.ok) { toast('Invalid address format', 'err', v.message); return; }
  if (!label) { toast('Label required', 'warn'); return; }
  const body = { chain, address, label };
  const r = await post('/v2/wallet/addresses', body);
  if (r.status === 200) {
    toast('Address added', 'ok', `${chain.toUpperCase()} · ${label} · enters 24h cool-down`);
    notifyEvent('address_added','address whitelisted',`${chain} · ${label} · ${address.slice(0,16)}…`);
  } else if (r.status === 409 || r.status === 422) {
    toast('Address rejected', 'err', `${r.status} — sanctions hit or duplicate`);
    notifyEvent('address_suspended','address rejected at add-time',`${chain} · HTTP ${r.status}`);
  } else {
    toast('Add failed', 'err', `HTTP ${r.status}`);
  }
  userListAddrs();
}
export async function userListAddrs() {
  const r = await get('/v2/wallet/addresses', { silent:true });
  const arr = Array.isArray(r.json) ? r.json : (r.json?.addresses || []);
  if (arr.length === 0) { $('addr_grid').innerHTML = '<em>no addresses yet</em>'; return; }
  $('addr_grid').innerHTML = `<div class="addr-grid">${
    arr.map(a => {
      const assets = (CHAIN_ASSETS[a.chain] || []).map(x => x.symbol).join(' · ');
      return `<div class="addr-card">
        <span class="chain">${a.chain}</span>
        <span class="pill" style="float:right">${a.status}</span>
        <div class="label">${escapeHtml(a.label || '(unnamed)')}</div>
        <div class="addr">${escapeHtml(a.address)}</div>
        <div style="font-size:11px;color:var(--mute);margin-top:4px">serves: ${assets || '—'}</div>
        <div class="meta">
          <span>added ${(a.added_at||'').slice(0,10)}</span>
          <button class="danger" onclick="userDelAddr('${a.address_id}')" style="padding:2px 8px;font-size:11px">delete</button>
        </div>
      </div>`;
    }).join('')
  }</div>`;
}
export async function userDelAddr(id) {
  const ok = await confirmModal({ title:'Remove address?', body:`Remove address <code>${escapeHtml(id.slice(0,18))}</code>?<br/><br/>The record is retained for audit but can no longer be a withdrawal destination. Re-adding requires a fresh 24h cool-down.`, okLabel:'Remove', danger:true });
  if (!ok) return;
  const r = await del_('/v2/wallet/addresses/' + encodeURIComponent(id));
  toast(r.status === 200 ? 'Address removed' : 'Remove failed', r.status === 200 ? 'ok' : 'err');
  userListAddrs();
}

window.__histFilter = 'all';
export function renderUserHistory(c) {
  c.innerHTML = `
    <h2>History <small>unified activity timeline</small></h2>
    <div class="card">
      <div class="filter-pills" id="hist_pills">
        <a data-f="all" class="active">all</a><a data-f="trades">trades</a><a data-f="withdrawals">withdrawals</a><a data-f="orders">orders</a>
      </div>
      <div class="row btns">
        <label>market for trades</label>${marketSelect('hist_mkt', DEFAULT_MARKET)}
        <button class="primary" onclick="userHistLoad()">refresh</button>
      </div>
      <div id="hist_timeline" style="margin-top:10px">loading…</div>
    </div>`;
  document.querySelectorAll('#hist_pills a').forEach(a => a.addEventListener('click', () => {
    document.querySelectorAll('#hist_pills a').forEach(x => x.classList.remove('active'));
    a.classList.add('active');
    window.__histFilter = a.dataset.f;
    userHistLoad();
  }));
  userHistLoad();
}
export async function userHistLoad() {
  const subj = $('subject').value;
  const m = $('hist_mkt').value;
  const [w, o, t] = await Promise.all([
    get('/v2/wallet/withdrawals', { silent:true }),
    get('/orders/' + encodeURIComponent(subj), { silent:true }),
    get(`/markets/${encodeURIComponent(m)}/trades`, { silent:true }),
  ]);
  const wArr = Array.isArray(w.json) ? w.json : (w.json?.withdrawals || []);
  const oArr = Array.isArray(o.json) ? o.json : (o.json?.orders || []);
  const tArr = Array.isArray(t.json) ? t.json : (t.json?.trades || []);
  const events = [];
  if (window.__histFilter === 'all' || window.__histFilter === 'withdrawals') {
    wArr.forEach(x => events.push({ ts: x.submitted_at, type:'wd', label:`${x.status} · ${(x.chain||'').toUpperCase()} ${x.amount} → ${(x.destination_address||'').slice(0,18)}…`, detail: x.withdrawal_id }));
  }
  if (window.__histFilter === 'all' || window.__histFilter === 'orders') {
    oArr.forEach(x => events.push({ ts: x.submitted_at || x.created_at, type:'trd', label:`${x.side} ${x.amount} @ ${x.price} ${x.market_id || x.market} · ${x.status}`, detail: x.order_id }));
  }
  if (window.__histFilter === 'all' || window.__histFilter === 'trades') {
    tArr.forEach(x => events.push({ ts: x.timestamp, type:'trd', label:`fill ${x.side} ${x.quantity} @ ${x.price} on ${m}`, detail:'' }));
  }
  events.sort((a,b) => (b.ts||'').localeCompare(a.ts||''));
  $('hist_timeline').innerHTML = events.length === 0 ? '<em>nothing matching filter</em>' : events.slice(0,80).map(e => `
    <div class="timeline-row">
      <div class="icon ${e.type}">${e.type==='wd'?'↑':e.type==='dep'?'↓':'↔'}</div>
      <div class="ts">${(e.ts||'').slice(0,19).replace('T',' ')}</div>
      <div>${escapeHtml(e.label)} ${e.detail ? `<span style="color:var(--mute);font-size:11px"> · ${e.detail.slice(0,28)}</span>` : ''}</div>
    </div>`).join('');
}

export function renderUserApiKeys(c) {
  const subj = $('subject').value;
  const stubKeys = JSON.parse(localStorage.getItem('uiApiKeys:'+subj) || '[]');
  c.innerHTML = `
    <h2>API Keys <small>HMAC credentials for programmatic access</small></h2>
    ${stub('No <code>/v2/auth/api-keys</code> endpoint yet — this page persists keys in <code>localStorage</code> for UI scaffolding.', '/v2/auth/api-keys')}
    <div class="card">
      <h3>create new key</h3>
      <div class="row">
        <label>label</label>${input('ak_label','market-maker-bot')}
        <label>scope</label>${select('ak_scope',['read-only','trade','withdraw','admin'])}
        <label>expires (days)</label>${input('ak_exp','90')}
        <button class="primary" onclick="userApiKeyCreate()">mint</button>
      </div>
      <small style="color:var(--mute)">A new HMAC pair is generated client-side and shown <strong>once</strong>. Store the secret in your password manager — the UI cannot recover it.</small>
      <div id="ak_new_out"></div>
    </div>
    <div class="card">
      <h3>existing keys <button onclick="renderUserApiKeys(document.getElementById('content'))" style="float:right;font-size:11px;padding:2px 8px">refresh</button></h3>
      ${stubKeys.length === 0 ? '<em>none yet</em>' : table(
        ['id','label','scope','created','expires','last used',''],
        stubKeys.map(k => [
          k.id.slice(0,18), escapeHtml(k.label), `<span class="pill">${k.scope}</span>`,
          k.created.slice(0,10), k.expires.slice(0,10),
          k.last_used ? k.last_used.slice(0,10) : '—',
          `<button class="danger" onclick="userApiKeyRevoke('${k.id}')" style="padding:2px 8px;font-size:11px">revoke</button>`,
        ]))}
    </div>`;
}
export async function userApiKeyCreate() {
  const subj = $('subject').value;
  const id = 'ak-' + uuidv4();
  const buf = new Uint8Array(32); crypto.getRandomValues(buf);
  const secret = btoa(String.fromCharCode(...buf));
  const days = parseInt($('ak_exp').value, 10) || 90;
  const rec = { id, label: $('ak_label').value, scope: $('ak_scope').value,
    created: new Date().toISOString(),
    expires: new Date(Date.now() + days*86400_000).toISOString(),
    last_used: null };
  const list = JSON.parse(localStorage.getItem('uiApiKeys:'+subj) || '[]');
  list.unshift(rec);
  localStorage.setItem('uiApiKeys:'+subj, JSON.stringify(list));
  $('ak_new_out').innerHTML = `
    <div class="summary-card" style="border-left-color:var(--ok)">
      <strong>✓ minted</strong> — copy now, won't be shown again.
      <dl class="kv" style="margin-top:6px">
        <dt>key id</dt><dd style="font-family:ui-monospace,Menlo,Consolas,monospace">${id}</dd>
        <dt>secret</dt><dd style="font-family:ui-monospace,Menlo,Consolas,monospace;word-break:break-all">${escapeHtml(secret)}</dd>
        <dt>scope</dt><dd>${rec.scope}</dd>
        <dt>expires</dt><dd>${rec.expires.slice(0,10)}</dd>
      </dl>
    </div>`;
  toast('API key minted', 'ok', 'copy the secret now — it will not be shown again');
  notifyEvent('security_alert','API key minted',`scope=${rec.scope} expires=${rec.expires.slice(0,10)}`);
  setTimeout(() => renderUserApiKeys($('content')), 800);
}
export async function userApiKeyRevoke(id) {
  const ok = await confirmModal({ title:'Revoke API key?', body:`Revoke <code>${escapeHtml(id)}</code>?<br/><br/>Any program using this key will start receiving 401 immediately.`, okLabel:'Revoke', danger:true });
  if (!ok) return;
  const subj = $('subject').value;
  const list = JSON.parse(localStorage.getItem('uiApiKeys:'+subj) || '[]').filter(k => k.id !== id);
  localStorage.setItem('uiApiKeys:'+subj, JSON.stringify(list));
  toast('API key revoked', 'warn');
  notifyEvent('security_alert','API key revoked', id);
  renderUserApiKeys($('content'));
}

export function renderUserNotifications(c) {
  const subj = $('subject').value;
  const prefs = JSON.parse(localStorage.getItem('uiNotifPrefs:'+subj) || '{}');
  const has = (k) => prefs[k] === undefined ? true : !!prefs[k];
  c.innerHTML = `
    <h2>Notifications <small>delivery channels + event subscriptions</small></h2>
    ${stub('No <code>/v2/notifications/*</code> endpoints yet.', '/v2/notifications')}
    <div class="split">
      <div class="card">
        <h3>delivery channels</h3>
        <div class="row"><label>email</label>${input('nf_email', prefs.email || (subj+'@example.test'))}</div>
        <div class="row"><label>webhook url</label>${input('nf_webhook', prefs.webhook || '')}</div>
        <small style="color:var(--mute)">Webhooks receive a JSON POST signed with the same HMAC scheme used by <code>/v2/wallet/*</code>.</small>
      </div>
      <div class="card">
        <h3>event subscriptions</h3>
        ${[
          ['withdrawal_settled','withdrawal settled'],
          ['withdrawal_awaiting','withdrawal awaiting maker-checker'],
          ['withdrawal_rejected','withdrawal rejected'],
          ['address_added','new address added'],
          ['address_suspended','address compliance-suspended'],
          ['order_filled','order filled'],
          ['low_balance','low balance threshold'],
          ['security_alert','security alert (sign-in / api key)'],
        ].map(([k,label]) => `
          <div class="row" style="border-bottom:1px solid var(--border);padding-bottom:4px">
            <label style="width:auto;flex:1">${label}</label>
            <label style="width:auto;color:var(--fg)"><input type="checkbox" id="nf_${k}" ${has(k)?'checked':''}/> notify</label>
          </div>`).join('')}
      </div>
    </div>
    <div class="card">
      <div class="row btns">
        <button class="primary" onclick="userNotifSave()">save preferences</button>
        <button onclick="userNotifTest()">send test event</button>
        <small id="nf_status" style="margin-left:auto;color:var(--mute)"></small>
      </div>
    </div>
    <div class="card"><h3>recent feed</h3><div id="nf_feed">${renderNotifFeedFromLocal(subj)}</div></div>`;
}
function renderNotifFeedFromLocal(subj) {
  const feed = JSON.parse(localStorage.getItem('uiNotifFeed:'+subj) || '[]');
  if (feed.length === 0) return '<em>nothing yet — interact with deposit / withdraw / orders to populate</em>';
  return feed.slice(0,30).map(e => `
    <div class="timeline-row">
      <div class="icon ${e.kind==='security_alert'?'wd':'trd'}">${e.kind==='withdrawal_settled'?'✓':e.kind==='security_alert'?'!':'·'}</div>
      <div class="ts">${(e.ts||'').slice(11,19)}</div>
      <div><strong>${escapeHtml(e.event)}</strong> <span style="color:var(--mute)">${escapeHtml(e.detail||'')}</span></div>
    </div>`).join('');
}
export function userNotifSave() {
  const subj = $('subject').value;
  const prefs = { email: $('nf_email').value, webhook: $('nf_webhook').value };
  ['withdrawal_settled','withdrawal_awaiting','withdrawal_rejected','address_added','address_suspended','order_filled','low_balance','security_alert']
    .forEach(k => { prefs[k] = $('nf_'+k).checked; });
  localStorage.setItem('uiNotifPrefs:'+subj, JSON.stringify(prefs));
  $('nf_status').textContent = 'saved (local) ' + new Date().toISOString().slice(11,19);
  toast('Preferences saved', 'ok', 'persisted to localStorage — backend wiring pending');
}
export function userNotifTest() {
  const subj = $('subject').value;
  const feed = JSON.parse(localStorage.getItem('uiNotifFeed:'+subj) || '[]');
  feed.unshift({ ts: new Date().toISOString(), kind:'security_alert', event:'test event', detail:'manually triggered from console' });
  localStorage.setItem('uiNotifFeed:'+subj, JSON.stringify(feed));
  $('nf_feed').innerHTML = renderNotifFeedFromLocal(subj);
  $('nf_status').textContent = 'test event added at ' + new Date().toISOString().slice(11,19);
}

export function renderUserSecurity(c) {
  c.innerHTML = `
    <h2>Security <small>session + HMAC</small></h2>
    <div class="stats">
      <div class="stat"><div class="label">subject</div><div class="val" style="font-size:14px">${escapeHtml($('subject').value)}</div></div>
      <div class="stat"><div class="label">role</div><div class="val" style="font-size:14px">${escapeHtml($('role').value)}</div></div>
      <div class="stat"><div class="label">secret</div><div class="val" style="font-size:14px;color:var(--mute)">${escapeHtml($('secret').value).slice(0,8)}…</div></div>
      <div class="stat"><div class="label">scheme</div><div class="val" style="font-size:14px">HMAC-SHA256</div></div>
    </div>
    <div class="card">
      <h3>canonical payload</h3>
      <div class="desc">Every request signs <code>METHOD\\nPATH\\nQUERY\\nSUBJECT\\nROLE\\nSESSION\\nTIMESTAMP\\nREQUEST_ID</code>.</div>
      ${form([
        ['method', select('sec_method',['GET','POST','DELETE'])],
        ['path', input('sec_path','/v2/wallet/withdrawals')],
        ['query', input('sec_query','')],
      ], `<button class="primary" onclick="userSecPreview()">preview</button> <button onclick="userSecCall()">send for real</button>`)}
      <div id="sec_out"></div>
    </div>
    <div class="card"><h3>migration plan</h3><div class="desc">HMAC-in-browser is the v1 scheme. Production cutover (gate <strong>P3-SEC-1</strong>) replaces this with an OIDC session JWT + CSRF cookie.</div></div>`;
}
export async function userSecPreview() {
  const subject = $('subject').value, role = $('role').value;
  const ts2 = Math.floor(Date.now()/1000);
  const rid = uuidv4();
  const payload = `${$('sec_method').value}\n${$('sec_path').value}\n${$('sec_query').value}\n${subject}\n${role}\n\n${ts2}\n${rid}`;
  const sig = await hmacHex($('secret').value, payload);
  showResult('sec_out', renderJSON({ canonical_payload: payload, signature: sig, headers: { 'x-internal-auth-subject': subject, 'x-internal-auth-role': role, 'x-internal-auth-timestamp': ts2, 'x-internal-auth-signature': sig } }));
}
export async function userSecCall() {
  const m = $('sec_method').value, p = $('sec_path').value + ($('sec_query').value ? '?' + $('sec_query').value : '');
  const r = await call(m, p, '');
  showResult('sec_out', renderJSON({ status: r.status, body: r.json || r.text }));
}
