// js/core.js — primitives + DOM helpers + catalog + toolkit.
// Imported by every page module + main.js.

// ─── DOM + log helpers ─────────────────────────────────────────────
export const $ = id => document.getElementById(id);
export function ts() { return new Date().toISOString().slice(11,23); }
export function clearLog() { const el = $('log'); if (el) el.innerHTML = ''; }
export function logRaw(html) { const el = $('log'); if (!el) return; el.innerHTML += html + '\n'; el.scrollTop = el.scrollHeight; }
export function logReq(method, path) { logRaw(`<span class="ts">${ts()}</span> <span class="req">→ ${method} ${path}</span>`); }
export function logResp(status, body) {
  const cls = status >= 200 && status < 300 ? 'ok' : 'err';
  logRaw(`<span class="ts">${ts()}</span> <span class="${cls}">← ${status}</span> ${escapeHtml((body||'').slice(0,4000))}`);
}
export function logErr(msg) { logRaw(`<span class="ts">${ts()}</span> <span class="err">!! ${escapeHtml(msg)}</span>`); }
export function escapeHtml(s) {
  if (s === null || s === undefined) return '';
  return String(s).replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'})[c]);
}

// ─── HMAC + signing primitives ─────────────────────────────────────
export async function hmacHex(secret, payload) {
  const enc = new TextEncoder();
  const key = await crypto.subtle.importKey('raw', enc.encode(secret), { name:'HMAC', hash:'SHA-256' }, false, ['sign']);
  const sig = await crypto.subtle.sign('HMAC', key, enc.encode(payload));
  return [...new Uint8Array(sig)].map(b => b.toString(16).padStart(2,'0')).join('');
}
export async function sha256Hex(bytes) {
  const buf = await crypto.subtle.digest('SHA-256', bytes);
  return [...new Uint8Array(buf)].map(b => b.toString(16).padStart(2,'0')).join('');
}
export function uuidv4() {
  return ([1e7]+-1e3+-4e3+-8e3+-1e11).replace(/[018]/g, c =>
    (c ^ crypto.getRandomValues(new Uint8Array(1))[0] & 15 >> c/4).toString(16));
}

// ─── HTTP wrapper ──────────────────────────────────────────────────
export async function call(method, path, body, opts = {}) {
  const base = $('base').value.replace(/\/$/,'');
  const secret = $('secret').value;
  const subject = opts.subject || $('subject').value;
  const role = opts.role || $('role').value;
  // Pre-flight: backend rejects unsigned/empty requests with ApiError(401),
  // which the warp recovery surfaces as HTTP 500 (server bug). Refuse to issue
  // the request rather than spew "Internal Server Error" into the console.
  if (!subject || !role || !secret) {
    if (!opts.silent) logErr(`signing skipped: empty ${!subject?'subject':!role?'role':'secret'} — set the identity controls (Dev mode)`);
    return { status: 0, error: 'unsigned: empty identity' };
  }
  if (secret.length < 32) {
    if (!opts.silent) logErr(`signing skipped: secret must be ≥ 32 chars (got ${secret.length})`);
    return { status: 0, error: 'unsigned: short secret' };
  }
  const sessionId = '';
  const timestamp = Math.floor(Date.now()/1000).toString();
  const requestId = uuidv4();
  const u = new URL(base + path);
  const query = u.search.replace(/^\?/,'');
  const payload = `${method}\n${u.pathname}\n${query}\n${subject}\n${role}\n${sessionId}\n${timestamp}\n${requestId}`;
  const signature = await hmacHex(secret, payload);
  const bodyBytes = body ? new TextEncoder().encode(body) : new Uint8Array(0);
  const bodyHash = await sha256Hex(bodyBytes);
  const headers = {
    'content-type': 'application/json',
    'x-request-id': requestId,
    'x-internal-auth-subject': subject,
    'x-internal-auth-role': role,
    'x-internal-auth-session-id': sessionId,
    'x-internal-auth-timestamp': timestamp,
    'x-internal-auth-signature': signature,
    'x-internal-auth-body-sha256': bodyHash,
  };
  if (!opts.silent) logReq(method, path);
  try {
    const resp = await fetch(u.toString(), { method, headers, body: body || undefined });
    const text = await resp.text();
    if (!opts.silent) logResp(resp.status, text);
    let json;
    try { json = JSON.parse(text); } catch {}
    return { status: resp.status, text, json };
  } catch (e) {
    if (!opts.silent) logErr(e.message);
    return { status: 0, error: e.message };
  }
}
export const get  = (p,o)=>call('GET', p, '', o);
export const post = (p,b,o)=>call('POST', p, JSON.stringify(b), o);
export const del_ = (p,o)=>call('DELETE', p, '', o);
export function asAdmin(fn) { return async (...args) => {
  const prev = { s: $('subject').value, r: $('role').value };
  $('subject').value = 'admin-test'; $('role').value = 'admin';
  try { return await fn(...args); }
  finally { $('subject').value = prev.s; $('role').value = prev.r; }
}; }

// ─── render helpers ────────────────────────────────────────────────
export function el(html) { const t = document.createElement('template'); t.innerHTML = html.trim(); return t.content.firstElementChild; }
export function form(rows, btns) {
  const inner = rows.map(([label, input]) => `<div class="row"><label>${label}</label>${input}</div>`).join('');
  const buttons = `<div class="row btns">${btns}</div>`;
  return inner + buttons;
}
export function input(id, val='', ph='') { return `<input id="${id}" value="${escapeHtml(val)}" placeholder="${escapeHtml(ph)}" />`; }
export function select(id, opts) { return `<select id="${id}">${opts.map(o => `<option>${o}</option>`).join('')}</select>`; }
export function textarea(id, val='') { return `<textarea id="${id}">${escapeHtml(val)}</textarea>`; }
export function table(headers, rows) {
  const head = headers.map(h => `<th>${h}</th>`).join('');
  const body = rows.map(r => `<tr>${r.map(c => `<td>${c == null ? '' : c}</td>`).join('')}</tr>`).join('');
  return `<table><thead><tr>${head}</tr></thead><tbody>${body}</tbody></table>`;
}
export function stub(text, suggestedPath) {
  return `<div class="stub"><strong>endpoint pending.</strong> ${text} ${suggestedPath ? `Suggested: <code>${suggestedPath}</code>.` : ''}</div>`;
}
export function renderJSON(j) { return `<pre style="background:#010409;border:1px solid var(--border);padding:8px;border-radius:3px;font-size:11px;white-space:pre-wrap">${escapeHtml(JSON.stringify(j, null, 2))}</pre>`; }
export function showResult(elId, data) { const e = document.getElementById(elId); if (e) e.innerHTML = data; }

// ─── market + chain catalog ────────────────────────────────────────
export const MARKETS = ['btc-usdt', 'eth-usdt', 'usdc-usdt'];
export const DEFAULT_MARKET = MARKETS[0];
export const CHAIN_ASSETS = {
  eth: [
    { symbol:'ETH',  label:'Ether (native)',         standard:'native', decimals:18, contract:'' },
    { symbol:'USDT', label:'Tether USD (ERC-20)',    standard:'erc20',  decimals:6,  contract:'0xdAC17F958D2ee523a2206206994597C13D831ec7' },
    { symbol:'USDC', label:'USD Coin (ERC-20)',      standard:'erc20',  decimals:6,  contract:'0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48' },
  ],
  btc: [
    { symbol:'BTC',  label:'Bitcoin (native)',       standard:'native', decimals:8,  contract:'' },
  ],
  sol: [
    { symbol:'SOL',  label:'Solana (native)',        standard:'native', decimals:9,  contract:'' },
    { symbol:'USDC', label:'USD Coin (SPL)',         standard:'spl',    decimals:6,  contract:'EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v' },
  ],
};
export function marketSelect(id, current = DEFAULT_MARKET, onchange = '') {
  return `<select id="${id}"${onchange ? ` onchange="${onchange}"` : ''}>${MARKETS.map(m => `<option ${m===current?'selected':''}>${m}</option>`).join('')}</select>`;
}
export function chainSelect(id, current = 'eth', onchange = '') {
  return `<select id="${id}"${onchange ? ` onchange="${onchange}"` : ''}>${Object.keys(CHAIN_ASSETS).map(c => `<option value="${c}" ${c===current?'selected':''}>${c.toUpperCase()}</option>`).join('')}</select>`;
}
export function assetOptionsForChain(chain) {
  return (CHAIN_ASSETS[chain] || []).map(a =>
    `<option value="${a.symbol}" data-std="${a.standard}" data-contract="${a.contract}" data-decimals="${a.decimals}">${a.symbol} — ${a.label}</option>`).join('');
}
export function repaintAssetSelect(chainSelId, assetSelId) {
  const ch = $(chainSelId)?.value || 'eth';
  const sel = $(assetSelId);
  if (!sel) return;
  sel.innerHTML = assetOptionsForChain(ch);
}
export function validateAddress(chain, address) {
  if (!address) return { ok:false, message:'address required' };
  if (chain === 'eth') {
    if (!/^0x[0-9a-fA-F]{40}$/.test(address)) return { ok:false, message:'ETH addresses must be 0x + 40 hex chars (EIP-55 not enforced here).' };
  } else if (chain === 'btc') {
    const legacy = /^[13][1-9A-HJ-NP-Za-km-z]{25,34}$/.test(address);
    const segwit = /^bc1[ac-hj-np-z02-9]{11,71}$/.test(address);
    if (!legacy && !segwit) return { ok:false, message:'BTC addresses must be legacy (1.../3...) or bech32 (bc1...).' };
  } else if (chain === 'sol') {
    if (!/^[1-9A-HJ-NP-Za-km-z]{32,44}$/.test(address)) return { ok:false, message:'SOL addresses must be base58 (32–44 chars).' };
  }
  return { ok:true, message:'looks valid' };
}

// ─── toolkit: toast / modal / palette / mode / persist / badges ───
export function toast(msg, level = 'ok', sub = '', ms = 3500) {
  const host = $('toast_host'); if (!host) return;
  const el = document.createElement('div');
  el.className = 'toast ' + (level === 'err' ? 'err' : level === 'warn' ? 'warn' : level === 'info' ? '' : 'ok');
  el.innerHTML = `<div>${escapeHtml(msg)}</div>${sub ? `<small>${escapeHtml(sub)}</small>` : ''}`;
  host.appendChild(el);
  setTimeout(() => { el.style.transition = 'opacity .25s'; el.style.opacity = '0'; setTimeout(() => el.remove(), 280); }, ms);
}
export function confirmModal({ title, body, okLabel = 'Confirm', okClass = 'primary', danger = false }) {
  return new Promise(resolve => {
    const host = $('modal_host'); if (!host) { resolve(true); return; }
    const back = document.createElement('div');
    back.className = 'modal-back';
    back.innerHTML = `<div class="modal" role="dialog" aria-modal="true">
      <h3>${escapeHtml(title)}</h3>
      <div class="body">${body}</div>
      <div class="actions">
        <button id="modalCancel">Cancel</button>
        <button id="modalOk" class="${danger ? 'danger' : okClass}">${escapeHtml(okLabel)}</button>
      </div>
    </div>`;
    host.appendChild(back);
    const finish = (v) => { back.remove(); document.removeEventListener('keydown', onKey); resolve(v); };
    const onKey = (e) => { if (e.key === 'Escape') finish(false); if (e.key === 'Enter') finish(true); };
    document.addEventListener('keydown', onKey);
    back.querySelector('#modalCancel').onclick = () => finish(false);
    back.querySelector('#modalOk').onclick = () => finish(true);
    back.addEventListener('click', e => { if (e.target === back) finish(false); });
    setTimeout(() => back.querySelector('#modalOk').focus(), 0);
  });
}
export function openPalette() {
  const host = $('palette_host'); if (!host) return;
  if (host.querySelector('.palette-back')) return;
  const PAGES = window.PAGES || {};
  const items = [];
  for (const [app, pages] of Object.entries(PAGES)) {
    if (!modeAllowsApp(app)) continue;
    for (const [slug, { title }] of Object.entries(pages)) {
      items.push({ app, slug, title, hash: `#${app}/${slug}`, key: `${app} ${title} ${slug}`.toLowerCase() });
    }
  }
  let q = '', idx = 0;
  const back = document.createElement('div');
  back.className = 'palette-back';
  back.innerHTML = `<div class="palette" role="dialog" aria-modal="true">
    <input id="palQ" placeholder="jump to page — type to filter, ↑↓ to navigate, ↵ to go" autocomplete="off"/>
    <ul id="palList"></ul>
  </div>`;
  host.appendChild(back);
  const close = () => { back.remove(); document.removeEventListener('keydown', onKey); };
  const render = () => {
    const matches = items.filter(it => !q || it.key.includes(q));
    if (idx >= matches.length) idx = 0;
    if (idx < 0) idx = Math.max(matches.length - 1, 0);
    back.querySelector('#palList').innerHTML = matches.slice(0, 40).map((it, i) => `
      <li class="${i === idx ? 'active' : ''}" data-i="${i}">
        <span class="app">${it.app}</span>
        <span class="title">${escapeHtml(it.title)}</span>
        <span class="hint">${it.hash}</span>
      </li>`).join('') || '<li><em style="color:var(--mute);padding:0 6px">no match</em></li>';
    back.querySelectorAll('li[data-i]').forEach(li => li.onclick = () => { go(matches[li.dataset.i]); });
    window.__palMatches = matches;
  };
  const go = (it) => { if (it) { location.hash = it.hash; close(); } };
  const onKey = (e) => {
    if (e.key === 'Escape') { close(); e.preventDefault(); }
    else if (e.key === 'ArrowDown') { idx++; render(); e.preventDefault(); }
    else if (e.key === 'ArrowUp')   { idx--; render(); e.preventDefault(); }
    else if (e.key === 'Enter')     { go((window.__palMatches||[])[idx]); e.preventDefault(); }
  };
  document.addEventListener('keydown', onKey);
  back.addEventListener('click', e => { if (e.target === back) close(); });
  back.querySelector('#palQ').addEventListener('input', e => { q = e.target.value.trim().toLowerCase(); idx = 0; render(); });
  render();
  setTimeout(() => back.querySelector('#palQ').focus(), 0);
}

export function persistIdentity() {
  ['subject','role','secret','base'].forEach(id => {
    const elx = $(id); if (!elx) return;
    const k = 'uiId:' + id;
    const stored = localStorage.getItem(k);
    if (stored !== null && stored !== '' && elx.value !== stored) elx.value = stored;
    // Only persist non-empty values — an accidental clear shouldn't poison the
    // identity across reloads (it'd surface as 500-from-401 on every signed call).
    const save = () => { const v = elx.value; if (v !== '' && v != null) localStorage.setItem(k, v); };
    elx.addEventListener('input',  save);
    elx.addEventListener('change', save);
  });
}

export async function refreshHealthAndBadges() {
  const h = await get('/health', { silent:true });
  if (h.json) {
    $('health').textContent = h.json.status; $('health').className = 'health ' + (h.json.status === 'ok' ? 'ok' : 'err');
    const up = parseInt(h.json.uptime_secs || 0, 10);
    const d = Math.floor(up/86400), hh = Math.floor((up%86400)/3600), mm = Math.floor((up%3600)/60), ss = up%60;
    $('uptime').textContent = (d ? d+'d ' : '') + String(hh).padStart(2,'0')+':'+String(mm).padStart(2,'0')+':'+String(ss).padStart(2,'0');
  } else { $('health').textContent='down'; $('health').className='health err'; $('uptime').textContent=''; }
  const mode = localStorage.getItem('uiMode') || 'user';
  if (mode === 'user') return;
  try {
    const [q, ap] = await Promise.all([
      asAdmin(()=>get('/admin/wallet/queue', { silent:true }))(),
      asAdmin(()=>get('/admin/approval-requests', { silent:true }))(),
    ]);
    const pending = (q.json?.pending || []).length;
    const stuck   = (q.json?.pending || []).filter(w => w.status === 'settlement_stuck').length;
    const await_  = (q.json?.pending || []).filter(w => w.status === 'awaiting_approval').length;
    const apvs    = ap.json?.pending || ap.json?.requests || (Array.isArray(ap.json)? ap.json:[]);
    setBadge('admin','queue', pending, stuck ? 'err' : await_ ? 'warn' : '');
    setBadge('admin','withdrawal_approval', await_, await_ ? 'warn' : '');
    setBadge('admin','approvals', apvs.length, apvs.length ? 'warn' : '');
  } catch {}
}
export function setBadge(app, slug, count, level = '') {
  window.__lastBadges = window.__lastBadges || {};
  window.__lastBadges[`${app}:${slug}`] = { count, level };
  const sel = `aside.side a.nav[href="#${app}/${slug}"]`;
  document.querySelectorAll(sel).forEach(a => {
    let b = a.querySelector('.badge');
    if (!count) { if (b) b.remove(); return; }
    if (!b) { b = document.createElement('span'); b.className = 'badge'; a.appendChild(b); }
    b.className = 'badge' + (level ? ' ' + level : '');
    b.textContent = count;
  });
}
export function toggleLogPane() {
  const layout = $('layout'); if (!layout) return;
  layout.classList.toggle('lognarrow');
  const btn = layout.querySelector('.collapse-btn'); if (btn) btn.textContent = layout.classList.contains('lognarrow') ? '«' : '»';
}
export function notifyEvent(kind, event, detail = '') {
  const subj = $('subject').value;
  const prefs = JSON.parse(localStorage.getItem('uiNotifPrefs:'+subj) || '{}');
  if (prefs[kind] === false) return;
  const feed = JSON.parse(localStorage.getItem('uiNotifFeed:'+subj) || '[]');
  feed.unshift({ ts: new Date().toISOString(), kind, event, detail });
  localStorage.setItem('uiNotifFeed:'+subj, JSON.stringify(feed.slice(0, 200)));
}
export function setMode(mode) {
  if (!['user','operator','developer'].includes(mode)) mode = 'user';
  document.body.classList.remove('mode-user','mode-operator','mode-developer');
  document.body.classList.add('mode-' + mode);
  document.querySelectorAll('#mode_toggle a, #mode_toggle_who a').forEach(a => a.classList.toggle('active', a.dataset.mode === mode));
  localStorage.setItem('uiMode', mode);
  const subj = $('subject')?.value || 'user-test-1';
  if ($('who_name')) $('who_name').textContent = subj;
  if (mode === 'user' && !location.hash.startsWith('#user/')) {
    location.hash = '#user/dashboard';
  }
}
export function modeAllowsApp(app) {
  const mode = localStorage.getItem('uiMode') || 'user';
  if (mode === 'user') return app === 'user';
  return true;
}

// Page-scoped timers — cleared on every route change.
window.__pageTimers = window.__pageTimers || [];
export function setPageInterval(fn, ms) {
  const id = setInterval(fn, ms);
  window.__pageTimers.push(id);
  return id;
}
export function clearPageTimers() {
  (window.__pageTimers || []).forEach(id => clearInterval(id));
  window.__pageTimers = [];
}
