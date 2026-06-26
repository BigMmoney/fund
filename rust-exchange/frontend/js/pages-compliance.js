// js/pages-compliance.js — Compliance app page renderers.
import {
  $, escapeHtml, get, post, call, asAdmin,
  form, input, select, table, stub, renderJSON, showResult,
  chainSelect, toast,
} from './core.js';

export function renderCompSanctions(c) {
  c.innerHTML = `
    <h2>Sanctions <small>provider status + recent screens</small></h2>
    ${stub('Provider screening runs at add-time + validate-time.', '/admin/sanctions/recent')}
    <div class="card">
      <h3>simulate add-time screen</h3>
      ${form([
        ['address', input('sx_addr','0xdest-aaa')],
        ['chain', chainSelect('sx_chain','eth')],
      ], `<button class="primary" onclick="compSxAdd()">POST /v2/wallet/addresses (acts as screen)</button>`)}
      <small style="color:var(--mute)">A clean ETH-chain screen covers ETH withdrawals AND ERC-20 (USDT/USDC) withdrawals to that destination — they share the address.</small>
      <div id="sx_out"></div>
    </div>`;
}
export async function compSxAdd() {
  const body = { chain: $('sx_chain').value, address: $('sx_addr').value, label: 'compliance-screen-test' };
  const r = await post('/v2/wallet/addresses', body);
  showResult('sx_out', renderJSON({status: r.status, body: r.json || r.text}));
}

export function renderCompSanctionsReview(c) {
  c.innerHTML = `
    <h2>Sanctions Review <small>provider hits + adjudication</small></h2>
    ${stub('No <code>/admin/compliance/sanctions/hits</code> endpoint yet — composes per-user reads.', '/admin/compliance/sanctions/hits')}
    <div class="card">
      <div class="row">
        <label>scan users</label>${input('sr_users','user-test-1, user-test-2, alice, bob')}
        <button class="primary" onclick="compSrLoad()">scan</button>
      </div>
      <div id="sr_stats" class="stats" style="margin-top:10px"></div>
      <div id="sr_table"></div>
    </div>
    <div class="card">
      <h3>adjudicate</h3>
      ${form([
        ['address_id', input('sr_addr_id','')],
        ['decision', select('sr_decision',['reactivate','keep_suspended','escalate'])],
        ['reason (≥16)', input('sr_reason','sanctions hit reviewed — false positive on entity disambiguation')],
      ], `<button class="primary" onclick="compSrAdjudicate()">file approval-request</button>`)}
      <div id="sr_act"></div>
    </div>`;
}
export async function compSrLoad() {
  const users = $('sr_users').value.split(',').map(s => s.trim()).filter(Boolean);
  if (users.length === 0) { toast('Provide at least one user', 'warn'); return; }
  const responses = await Promise.all(users.map(u =>
    call('GET','/v2/wallet/addresses','',{ subject:u, role:'user', silent:true }).then(r => ({ u, r }))));
  const all = [];
  for (const { u, r } of responses) {
    const arr = Array.isArray(r.json) ? r.json : (r.json?.addresses || []);
    arr.forEach(a => all.push({ user: u, ...a }));
  }
  const hits   = all.filter(a => a.sanctions_check?.status === 'hit' || a.status === 'suspended');
  const errors = all.filter(a => a.sanctions_check?.status === 'error');
  const clear  = all.filter(a => a.sanctions_check?.status === 'clear');
  const pending= all.filter(a => a.sanctions_check?.status === 'pending');
  $('sr_stats').innerHTML = `
    <div class="stat ${hits.length?'err':'ok'}"><div class="label">hits</div><div class="val">${hits.length}</div><div class="sub">blocked addresses</div></div>
    <div class="stat ${errors.length?'warn':'ok'}"><div class="label">errors</div><div class="val">${errors.length}</div><div class="sub">soft-blocked, retry pending</div></div>
    <div class="stat"><div class="label">pending</div><div class="val">${pending.length}</div><div class="sub">cool-down period</div></div>
    <div class="stat ok"><div class="label">clear</div><div class="val">${clear.length}</div><div class="sub">across ${users.length} user(s)</div></div>`;
  if (hits.length === 0 && errors.length === 0) { $('sr_table').innerHTML = '<em>no hits or errors in this scan</em>'; return; }
  const rows = [...hits, ...errors].map(a => {
    const sc = a.sanctions_check || {};
    return [
      `<button onclick="document.getElementById('sr_addr_id').value='${a.address_id}';scrollTo(0,9999)" style="padding:2px 8px;font-size:11px">pick</button>`,
      a.user, a.chain, escapeHtml(a.label || ''),
      `<span style="font-family:ui-monospace,Menlo,Consolas,monospace;font-size:11px">${(a.address||'').slice(0,28)}…</span>`,
      `<span class="pill ${sc.status==='hit'?'err':'warn'}">${sc.status}</span>`,
      sc.provider || '—',
      sc.hit ? `${sc.hit.list_name} (score ${sc.hit.score})` : '—',
      `<span class="pill ${a.status==='suspended'?'err':a.status==='active'?'ok':'warn'}">${a.status}</span>`,
    ];
  });
  $('sr_table').innerHTML = table(['','user','chain','label','address','screen','provider','match','address state'], rows);
}
export async function compSrAdjudicate() {
  const decision = $('sr_decision').value;
  const action = decision === 'reactivate' ? 'AddressReactivate' : decision === 'escalate' ? 'AddressEscalate' : 'AddressKeepSuspended';
  const body = { action, resource: { kind:'address', id: $('sr_addr_id').value }, scope: 'Global', reason: $('sr_reason').value, action_payload: { decision } };
  const r = await asAdmin(()=>post('/admin/approval-requests', body))();
  $('sr_act').innerHTML = `<dl class="kv"><dt>status</dt><dd>${r.status}</dd></dl>${renderJSON(r.json || r.text)}`;
}

export function renderCompReview(c) {
  c.innerHTML = `
    <h2>Manual Review <small>queue of flagged actions</small></h2>
    ${stub('No <code>/admin/compliance/review</code> endpoint yet.', '/admin/compliance/review')}
    <div class="card">
      <h3>awaiting-approval withdrawals (impersonated)</h3>
      ${form([['user', input('rv_user','user-test-1')]], `<button onclick="compRvLoad()">load</button>`)}
      <div id="rv_out"></div>
    </div>`;
}
export async function compRvLoad() {
  const u = $('rv_user').value;
  const r = await call('GET','/v2/wallet/withdrawals','',{subject:u,role:'user'});
  const arr = Array.isArray(r.json) ? r.json : (r.json?.withdrawals || []);
  const flagged = arr.filter(w => w.status === 'awaiting_approval' || w.status === 'settlement_stuck');
  showResult('rv_out', flagged.length ? table(['id','status','amount','dest'],
    flagged.map(w => [w.withdrawal_id.slice(0,28), `<span class="pill warn">${w.status}</span>`, w.amount, w.destination_address])) : '<em>nothing flagged for this user</em>');
}

export function renderCompSuspended(c) {
  c.innerHTML = `
    <h2>Suspended Addresses <small>per-user view</small></h2>
    <div class="card">${form([['user', input('su_user','user-test-1')]], `<button class="primary" onclick="compSuLoad()">load</button>`)}<div id="su_out"></div></div>
    ${stub('No admin-side cross-user filter yet.', '/admin/compliance/suspended-addresses')}`;
}
export async function compSuLoad() {
  const u = $('su_user').value;
  const r = await call('GET','/v2/wallet/addresses','',{subject:u,role:'user'});
  const arr = Array.isArray(r.json) ? r.json : (r.json?.addresses || []);
  const sus = arr.filter(a => ['suspended','pending_review','removed'].includes(a.status));
  showResult('su_out', sus.length ? table(['id','chain','address','label','status'],
    sus.map(a => [a.address_id.slice(0,28), a.chain, a.address, a.label, `<span class="pill warn">${a.status}</span>`])) : '<em>nothing flagged</em>');
}

export function renderCompReports(c) {
  c.innerHTML = `<h2>Reports <small>regulatory exports</small></h2>${stub('No reporting endpoint yet.', '/admin/compliance/reports')}`;
}
export function renderCompRetention(c) {
  c.innerHTML = `<h2>Retention / Export <small>data lifecycle</small></h2>${stub('Retention is currently "keep everything" — JSONL stores are append-only.', '/admin/compliance/export')}`;
}
