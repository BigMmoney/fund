// js/pages-admin.js — Admin app page renderers.
import {
  $, escapeHtml, get, post, call, asAdmin,
  form, input, select, textarea, table, stub, renderJSON, showResult, logErr,
  MARKETS, DEFAULT_MARKET, CHAIN_ASSETS, marketSelect,
  toast, confirmModal,
} from './core.js';

export function renderAdminOverview(c) {
  c.innerHTML = `
    <h2>Admin Overview <small>health + permissions</small></h2>
    <div class="card"><h3>health</h3><div id="adm_health"></div></div>
    <div class="card"><h3>my permissions <small>(GET /admin/me/permissions)</small></h3><div id="adm_perm"></div></div>`;
  (async () => {
    const h = await get('/health', { silent:true });
    showResult('adm_health', h.json ? renderJSON(h.json) : '<em>down</em>');
    const p = await asAdmin(()=> get('/admin/me/permissions', { silent:true }))();
    showResult('adm_perm', p.json ? renderJSON(p.json) : `<em>${p.status}</em>`);
  })();
}

export function renderAdminCustomers(c) {
  c.innerHTML = `
    <h2>Customers <small>balances + activity by user · BTC · ETH · USDT · USDC</small></h2>
    <div class="card">
      ${form([['user_id', input('cust_id','user-test-1')]], `<button onclick="admCustLoad()">load summary</button>`)}
      <small style="color:var(--mute)">Surfaces balances across all assets, open orders across ${MARKETS.join(' · ')}, and recent withdrawals across all chains for this user.</small>
      <div id="cust_out"></div>
    </div>
    ${stub('No <code>/admin/customers</code> endpoint exists yet — this page composes from per-user reads.', '/admin/customers')}`;
}
export async function admCustLoad() {
  const u = $('cust_id').value;
  const [bal, ords, wds] = await Promise.all([
    get('/balances/' + encodeURIComponent(u), { silent:true }),
    get('/orders/' + encodeURIComponent(u), { silent:true }),
    asAdmin(()=>get('/v2/wallet/withdrawals', { silent:true, subject:u, role:'user' }))(),
  ]);
  showResult('cust_out', `<h3>balances</h3>${bal.json ? renderJSON(bal.json) : '—'}<h3>orders</h3>${ords.json ? renderJSON(ords.json) : '—'}<h3>withdrawals</h3>${wds.json ? renderJSON(wds.json) : '—'}`);
}

window.__queueFilter = 'all';
export function renderAdminQueue(c) {
  c.innerHTML = `
    <h2>Wallet Queue <small>/admin/wallet/queue · approval / risk control</small></h2>
    <div class="card">
      <div class="filter-pills" id="q_pills">
        <a data-f="all" class="active">all chains</a>
        <a data-f="eth">eth + erc-20</a>
        <a data-f="btc">btc</a>
        <a data-f="sol">sol</a>
        <a data-f="awaiting_approval">awaiting approval</a>
        <a data-f="settlement_stuck">settlement stuck</a>
      </div>
      <div class="row btns">
        <button class="primary" onclick="admQueueLoad()">refresh</button>
        <small id="q_count" style="margin-left:auto;color:var(--mute)"></small>
      </div>
      <div id="queue_table"></div>
    </div>`;
  document.querySelectorAll('#q_pills a').forEach(a => a.addEventListener('click', () => {
    document.querySelectorAll('#q_pills a').forEach(x => x.classList.remove('active'));
    a.classList.add('active');
    window.__queueFilter = a.dataset.f;
    admQueueLoad();
  }));
  admQueueLoad();
}
export async function admQueueLoad() {
  const r = await asAdmin(()=> get('/admin/wallet/queue'))();
  if (!r.json) return;
  const pending = r.json.pending || [];
  const f = window.__queueFilter || 'all';
  const filtered = pending.filter(w => {
    if (f === 'all') return true;
    if (['eth','btc','sol'].includes(f)) return w.chain === f;
    return w.status === f;
  });
  const head = `<dl class="kv"><dt>total pending</dt><dd>${r.json.total ?? pending.length}</dd></dl>`;
  $('q_count').textContent = `${filtered.length} of ${pending.length} matching "${f}"`;
  showResult('queue_table', head + (filtered.length === 0 ? '<em>nothing matching filter</em>' : table(
    ['id','user','chain','asset','amount','status','submitted'],
    filtered.map(w => [
      w.withdrawal_id?.slice(0,28),
      w.user_id, w.chain,
      w.asset || (CHAIN_ASSETS[w.chain]?.[0]?.symbol ?? '—'),
      w.amount,
      `<span class="pill ${w.status==='awaiting_approval'?'warn':w.status==='settlement_stuck'?'err':''}">${w.status}</span>`,
      (w.submitted_at||'').slice(0,19),
    ]))));
}

export function renderAdminAddresses(c) {
  c.innerHTML = `
    <h2>Addresses <small>operator view of customer whitelists</small></h2>
    <div class="card">
      ${form([['as_subject', input('admaddr_user','user-test-1')]], `<button onclick="admAddrLoad()">load (impersonate)</button>`)}
      <div id="admaddr_out"></div>
    </div>
    ${stub('No admin-side multi-user listing endpoint yet — this page impersonates a subject.', '/admin/wallet/addresses')}`;
}
export async function admAddrLoad() {
  const u = $('admaddr_user').value;
  const r = await call('GET','/v2/wallet/addresses', '', { subject:u, role:'user' });
  const arr = Array.isArray(r.json) ? r.json : (r.json?.addresses || []);
  showResult('admaddr_out', arr.length === 0 ? '<em>none</em>' : table(['id','chain','address','label','status'],
    arr.map(a => [a.address_id.slice(0,28), a.chain, a.address, a.label, `<span class="pill">${a.status}</span>`])));
}

export function renderAdminApprovals(c) {
  c.innerHTML = `
    <h2>Approvals <small>maker-checker queue</small></h2>
    <div class="card">
      <h3>submit request</h3>
      <div class="row">
        <label>preset</label>
        <select id="apv_preset" onchange="admApvPreset()">
          <option value="halt-btc-usdt">halt market btc-usdt</option>
          <option value="halt-eth-usdt">halt market eth-usdt</option>
          <option value="halt-usdc-usdt">halt market usdc-usdt</option>
          <option value="resume-btc-usdt">resume market btc-usdt</option>
          <option value="approve-wd-eth">approve withdrawal (ETH/ERC-20)</option>
          <option value="approve-wd-btc">approve withdrawal (BTC)</option>
          <option value="reactivate-addr">re-activate suspended address</option>
          <option value="custom">custom</option>
        </select>
        <button onclick="admApvPreset()">apply</button>
      </div>
      ${form([
        ['action', input('apv_action','MarketHalt')],
        ['resource kind', input('apv_kind','market')],
        ['resource id', input('apv_id','btc-usdt')],
        ['scope', select('apv_scope',['Global','Market'])],
        ['reason (≥16 chars)', input('apv_reason','launching maker-checker drill from console')],
        ['payload (json)', textarea('apv_payload','{}')],
      ], `<button class="primary" onclick="admApvSubmit()">POST /admin/approval-requests</button>`)}
    </div>
    <div class="card">
      <h3>pending</h3>
      ${form([], `<button onclick="admApvList()">refresh</button>`)}
      <div id="apv_list"></div>
    </div>
    <div class="card">
      <h3>approve / reject</h3>
      ${form([
        ['request_id', input('apv_rid','')],
        ['as approver subject', input('apv_who','admin-test-2')],
        ['reason (≥16 chars)', input('apv_who_reason','approving via console for drill purposes')],
      ], `<button class="primary" onclick="admApvApprove()">approve</button> <button class="danger" onclick="admApvReject()">reject</button>`)}
    </div>`;
}
export function admApvPreset() {
  const p = $('apv_preset').value;
  const presets = {
    'halt-btc-usdt':   ['MarketHalt','market','btc-usdt','Market','halting btc-usdt for operational drill','{}'],
    'halt-eth-usdt':   ['MarketHalt','market','eth-usdt','Market','halting eth-usdt for operational drill','{}'],
    'halt-usdc-usdt':  ['MarketHalt','market','usdc-usdt','Market','halting usdc-usdt for operational drill','{}'],
    'resume-btc-usdt': ['MarketResume','market','btc-usdt','Market','resuming btc-usdt after drill completion','{}'],
    'approve-wd-eth':  ['WithdrawalsApprove','withdrawal','wd-id-here','Global','approving ETH-chain withdrawal — risk reviewed','{"chain":"eth","asset":"USDT"}'],
    'approve-wd-btc':  ['WithdrawalsApprove','withdrawal','wd-id-here','Global','approving BTC withdrawal — UTXOs available','{"chain":"btc","asset":"BTC"}'],
    'reactivate-addr': ['AddressReactivate','address','addr-id-here','Global','re-activating suspended address after compliance review cleared','{"chain":"eth"}'],
    'custom': null,
  };
  const v = presets[p]; if (!v) return;
  $('apv_action').value=v[0]; $('apv_kind').value=v[1]; $('apv_id').value=v[2];
  $('apv_scope').value=v[3]; $('apv_reason').value=v[4]; $('apv_payload').value=v[5];
}
export async function admApvSubmit() {
  let payload = {};
  try { payload = JSON.parse($('apv_payload').value || '{}'); } catch (e) { return logErr('payload parse: ' + e.message); }
  const body = { action: $('apv_action').value, resource: { kind: $('apv_kind').value, id: $('apv_id').value }, scope: $('apv_scope').value, reason: $('apv_reason').value, action_payload: payload };
  await asAdmin(()=>post('/admin/approval-requests', body))();
  admApvList();
}
export async function admApvList() {
  const r = await asAdmin(()=>get('/admin/approval-requests'))();
  const arr = r.json?.pending || r.json?.requests || (Array.isArray(r.json) ? r.json : []);
  showResult('apv_list', arr.length === 0 ? '<em>none pending</em>' : table(
    ['id','action','resource','submitter','status','expires'],
    arr.map(q => [q.approval_request_id?.slice(0,18), q.action, `${q.resource?.kind}:${q.resource?.id}`, q.submitter_employee_id, `<span class="pill">${q.status}</span>`, (q.expires_at||'').slice(0,19)])));
}
export async function admApvApprove() {
  const id = $('apv_rid').value, who = $('apv_who').value;
  if (!id) { toast('request_id required', 'warn'); return; }
  if (($('apv_who_reason').value||'').length < 16) { toast('Reason must be ≥ 16 chars', 'warn'); return; }
  const r = await call('POST', `/admin/approval-requests/${encodeURIComponent(id)}/approve`, JSON.stringify({reason:$('apv_who_reason').value}), {subject:who, role:'admin'});
  toast(r.status === 200 ? 'Approval recorded' : 'Approve failed', r.status === 200 ? 'ok' : 'err', `as ${who}`);
  admApvList();
}
export async function admApvReject() {
  const id = $('apv_rid').value, who = $('apv_who').value;
  if (!id) { toast('request_id required', 'warn'); return; }
  if (($('apv_who_reason').value||'').length < 16) { toast('Reason must be ≥ 16 chars', 'warn'); return; }
  const ok = await confirmModal({ title:'Reject approval request?', body:`Reject <code>${escapeHtml(id.slice(0,18))}</code> as <code>${escapeHtml(who)}</code>?`, okLabel:'Reject', danger:true });
  if (!ok) return;
  const r = await call('POST', `/admin/approval-requests/${encodeURIComponent(id)}/reject`, JSON.stringify({reason:$('apv_who_reason').value}), {subject:who, role:'admin'});
  toast(r.status === 200 ? 'Rejection recorded' : 'Reject failed', r.status === 200 ? 'warn' : 'err');
  admApvList();
}

export function renderAdminEmployees(c) {
  c.innerHTML = `
    <h2>Employees <small>/admin/employees</small></h2>
    <div class="card">${form([], `<button class="primary" onclick="admEmpLoad()">refresh</button>`)}<div id="emp_table"></div></div>
    ${stub('Read-only today.')}`;
  admEmpLoad();
}
export async function admEmpLoad() {
  const r = await asAdmin(()=>get('/admin/employees'))();
  const emps = r.json?.employees || [];
  showResult('emp_table', emps.length === 0 ? '<em>no employees — set BACKOFFICE_BOOTSTRAP_ADMIN</em>' : table(
    ['employee_id','status','display_name','last_login','active grants'],
    emps.map(e => [e.employee_id, `<span class="pill">${e.status}</span>`, e.display_name || '', (e.last_login_at||'').slice(0,19), (e.active_grants||[]).map(g => g.role).join(', ')])));
}

export function renderAdminGrants(c) {
  c.innerHTML = `
    <h2>Role Grants <small>derived from /admin/employees</small></h2>
    <div class="card">${form([], `<button class="primary" onclick="admGrLoad()">refresh</button>`)}<div id="gr_table"></div></div>
    ${stub("Reconstructed from each employee's active_grants.", '/admin/role-grants')}`;
  admGrLoad();
}
export async function admGrLoad() {
  const r = await asAdmin(()=>get('/admin/employees', { silent:true }))();
  const emps = r.json?.employees || [];
  const rows = [];
  for (const e of emps) for (const g of (e.active_grants||[])) {
    rows.push([e.employee_id, g.role, g.scope?.kind || g.scope, g.level, `<span class="pill">${g.status}</span>`, (g.expires_at||'').slice(0,19)]);
  }
  showResult('gr_table', rows.length === 0 ? '<em>no grants</em>' : table(['employee','role','scope','level','status','expires'], rows));
}

export function renderAdminMarketControls(c) {
  c.innerHTML = `
    <h2>Market Controls <small>halt / resume / state</small></h2>
    <div class="card">
      ${form([
        ['market', marketSelect('mc_market', DEFAULT_MARKET)],
        ['reason (≥16 chars)', input('mc_reason','operational halt drill from console')],
      ], `<button class="warn" onclick="admMktHalt()">halt</button> <button class="primary" onclick="admMktResume()">resume</button> <button onclick="admMktState()">state</button>`)}
      <div id="mc_out"></div>
    </div>`;
}
export async function admMktHalt() {
  const m = $('mc_market').value;
  const reason = $('mc_reason').value;
  if ((reason||'').length < 16) { toast('Reason must be ≥ 16 chars', 'warn'); return; }
  const ok = await confirmModal({ title:`Halt market ${m}?`, body:`Halt trading on <code>${escapeHtml(m)}</code>.`, okLabel:'Halt', danger:true });
  if (!ok) return;
  const r = await asAdmin(()=>post(`/admin/trading-ops/markets/${encodeURIComponent(m)}/halt`, { reason }))();
  toast(r.status === 200 ? `Halted ${m}` : 'Halt failed', r.status === 200 ? 'warn' : 'err');
}
export async function admMktResume() {
  const m = $('mc_market').value;
  const reason = $('mc_reason').value;
  if ((reason||'').length < 16) { toast('Reason must be ≥ 16 chars', 'warn'); return; }
  const r = await asAdmin(()=>post(`/admin/trading-ops/markets/${encodeURIComponent(m)}/resume`, { reason }))();
  toast(r.status === 200 ? `Resumed ${m}` : 'Resume failed', r.status === 200 ? 'ok' : 'err');
}
export async function admMktState() {
  const m = $('mc_market').value;
  const r = await asAdmin(()=>get(`/admin/market-state/${encodeURIComponent(m)}`))();
  showResult('mc_out', r.json ? renderJSON(r.json) : '');
}

export function renderAdminTransfers(c) {
  c.innerHTML = `
    <h2>Internal Transfers <small>operator-initiated journal</small></h2>
    ${stub('No dedicated /admin/transfers endpoint.', '/admin/transfers')}
    <div class="card">
      ${form([
        ['from user', input('tx_from','user-test-1')],
        ['to user', input('tx_to','user-test-2')],
        ['amount', input('tx_amount','1000')],
        ['op_id', input('tx_op','','auto')],
      ], `<button class="primary" onclick="admTxStub()">simulate (no-op)</button>`)}
      <div id="tx_out"></div>
    </div>`;
}
export async function admTxStub() {
  showResult('tx_out', `<em>would POST /admin/transfers { from:'${$('tx_from').value}', to:'${$('tx_to').value}', amount:${$('tx_amount').value} } once the endpoint lands.</em>`);
}

export function renderAdminAudit(c) {
  c.innerHTML = `
    <h2>Audit Logs <small>/admin/audit/actions</small></h2>
    <div class="card">${form([['limit', input('aud_limit','50')]], `<button class="primary" onclick="admAudLoad()">refresh</button>`)}<div id="aud_table"></div></div>`;
  admAudLoad();
}
export async function admAudLoad() {
  const r = await asAdmin(()=>get('/admin/audit/actions?limit=' + encodeURIComponent($('aud_limit').value)))();
  const items = r.json?.items || r.json?.entries || r.json?.actions || [];
  showResult('aud_table', items.length === 0 ? '<em>no audit rows</em>' : table(
    ['ts','subject','role','action','request_id'],
    items.map(a => [(a.recorded_at||a.timestamp||a.ts||'').slice(11,19), a.subject, a.role, a.action, (a.request_id||'').slice(0,12)])));
}

export function renderAdminCustomerDetail(c) {
  c.innerHTML = `
    <h2>Customer Detail <small>360° view of one user</small></h2>
    <div class="card">
      <div class="row">
        <label>user_id</label>${input('cd_user','user-test-1')}
        <button class="primary" onclick="admCustDetail()">load</button>
        <small style="margin-left:auto;color:var(--mute)">Composes per-user reads — would land at <code>GET /admin/customers/{id}</code>.</small>
      </div>
    </div>
    <div id="cd_stats" class="stats"></div>
    <div class="split">
      <div class="card"><h3>balances</h3><div id="cd_bal">—</div></div>
      <div class="card"><h3>open orders</h3><div id="cd_ords">—</div></div>
    </div>
    <div class="split">
      <div class="card"><h3>address book</h3><div id="cd_addrs">—</div></div>
      <div class="card"><h3>withdrawals</h3><div id="cd_wds">—</div></div>
    </div>
    <div class="card"><h3>operator actions on this customer (audit log filtered)</h3><div id="cd_audit">—</div></div>`;
}
export async function admCustDetail() {
  const u = $('cd_user').value;
  const [bal, ords, wds, addrs, aud] = await Promise.all([
    get('/balances/' + encodeURIComponent(u), { silent:true }),
    get('/orders/' + encodeURIComponent(u), { silent:true }),
    call('GET','/v2/wallet/withdrawals','',{ subject:u, role:'user', silent:true }),
    call('GET','/v2/wallet/addresses','', { subject:u, role:'user', silent:true }),
    asAdmin(()=>get('/admin/audit/actions?limit=200', { silent:true }))(),
  ]);
  const bArr = Array.isArray(bal.json) ? bal.json : (bal.json?.balances || []);
  const oArr = Array.isArray(ords.json) ? ords.json : (ords.json?.orders || []);
  const wArr = Array.isArray(wds.json) ? wds.json : (wds.json?.withdrawals || []);
  const aArr = Array.isArray(addrs.json) ? addrs.json : (addrs.json?.addresses || []);
  const audItems = aud.json?.items || aud.json?.entries || aud.json?.actions || [];
  const totalAvail = bArr.reduce((s,b)=>s+(parseInt(b.available,10)||0), 0);
  const totalHold  = bArr.reduce((s,b)=>s+(parseInt(b.hold,10)||0), 0);
  const openOrds   = oArr.filter(o => !['filled','cancelled','canceled','rejected','expired'].includes(o.status));
  const pendingWd  = wArr.filter(w => !['settled','rejected','removed'].includes(w.status));
  const flagged    = aArr.filter(a => ['suspended','pending_review'].includes(a.status));
  $('cd_stats').innerHTML = `
    <div class="stat"><div class="label">total balance</div><div class="val">${totalAvail.toLocaleString()}</div><div class="sub">${bArr.length} asset(s) · ${totalHold.toLocaleString()} hold</div></div>
    <div class="stat"><div class="label">open orders</div><div class="val">${openOrds.length}</div><div class="sub">${oArr.length} total</div></div>
    <div class="stat ${pendingWd.length?'warn':'ok'}"><div class="label">pending wd</div><div class="val">${pendingWd.length}</div><div class="sub">${wArr.length} total · ${wArr.filter(w=>w.status==='settled').length} settled</div></div>
    <div class="stat ${flagged.length?'err':'ok'}"><div class="label">addresses</div><div class="val">${aArr.length}</div><div class="sub">${flagged.length} flagged</div></div>`;
  $('cd_bal').innerHTML  = bArr.length === 0 ? '<em>none</em>' : table(['asset','available','hold'], bArr.map(b => [b.asset, parseInt(b.available,10)?.toLocaleString(), parseInt(b.hold,10)?.toLocaleString()]));
  $('cd_ords').innerHTML = openOrds.length === 0 ? '<em>none open</em>' : table(['id','market','side','px','qty','status'],
    openOrds.slice(0,10).map(o => [(o.order_id||'').slice(0,12), o.market_id||o.market, o.side, o.price, o.amount, `<span class="pill">${o.status}</span>`]));
  $('cd_addrs').innerHTML = aArr.length === 0 ? '<em>none</em>' : table(['chain','label','address','status'],
    aArr.map(a => [a.chain, escapeHtml(a.label||''), (a.address||'').slice(0,28)+'…', `<span class="pill ${['suspended','removed'].includes(a.status)?'err':a.status==='pending_cooldown'?'warn':'ok'}">${a.status}</span>`]));
  $('cd_wds').innerHTML = wArr.length === 0 ? '<em>none</em>' : table(['id','chain','asset','amount','status','submitted'],
    wArr.slice(0,15).map(w => [(w.withdrawal_id||'').slice(0,16), w.chain, w.asset || (CHAIN_ASSETS[w.chain]?.[0]?.symbol??'—'), w.amount, `<span class="pill ${w.status==='settled'?'ok':w.status==='rejected'?'err':'warn'}">${w.status}</span>`, (w.submitted_at||'').slice(0,19)]));
  const filtered = audItems.filter(a => JSON.stringify(a).includes(u)).slice(0,30);
  $('cd_audit').innerHTML = filtered.length === 0 ? '<em>no operator actions involving this user</em>' : table(
    ['ts','operator','role','action','request_id'],
    filtered.map(a => [(a.recorded_at||a.timestamp||a.ts||'').slice(0,19), a.subject, a.role, a.action, (a.request_id||'').slice(0,12)]));
}

export function renderAdminWithdrawalApproval(c) {
  c.innerHTML = `
    <h2>Withdrawal Approval <small>maker-checker queue for above-threshold + flagged withdrawals</small></h2>
    <div class="card">
      <div class="row btns"><button class="primary" onclick="admWdApvLoad()">refresh</button><small style="margin-left:auto;color:var(--mute)">Pulls from <code>GET /admin/wallet/queue</code> ∩ <code>/admin/approval-requests</code>.</small></div>
      <div id="wdapv_summary" class="stats"></div>
      <div id="wdapv_list"></div>
    </div>
    <div class="card">
      <h3>act on a withdrawal</h3>
      ${form([
        ['withdrawal_id', input('wdapv_id','')],
        ['approver subject', input('wdapv_who','admin-test-2')],
        ['reason (≥16)', input('wdapv_reason','approving after risk + balance review')],
      ], `<button class="primary" onclick="admWdApvAct('approve')">approve</button> <button class="danger" onclick="admWdApvAct('reject')">reject</button>`)}
      <div id="wdapv_act"></div>
    </div>`;
  admWdApvLoad();
}
export async function admWdApvLoad() {
  const [q, r] = await Promise.all([
    asAdmin(()=>get('/admin/wallet/queue', { silent:true }))(),
    asAdmin(()=>get('/admin/approval-requests', { silent:true }))(),
  ]);
  const pending = (q.json?.pending || []).filter(w => ['awaiting_approval','submitted','validated','queued'].includes(w.status));
  const apvs = r.json?.pending || r.json?.requests || (Array.isArray(r.json)? r.json : []);
  const wdApvs = apvs.filter(a => a.action === 'WithdrawalsApprove' || (a.resource?.kind === 'withdrawal'));
  const overTotal = pending.reduce((s,w)=>s + (parseInt(w.amount,10)||0), 0);
  const stuck = (q.json?.pending || []).filter(w => w.status === 'settlement_stuck').length;
  $('wdapv_summary').innerHTML = `
    <div class="stat warn"><div class="label">awaiting</div><div class="val">${pending.length}</div><div class="sub">${overTotal.toLocaleString()} aggregate</div></div>
    <div class="stat"><div class="label">linked approvals</div><div class="val">${wdApvs.length}</div><div class="sub">requests in MC queue</div></div>
    <div class="stat ${stuck?'err':'ok'}"><div class="label">settlement stuck</div><div class="val">${stuck}</div><div class="sub">on-chain confirmed, ledger pending</div></div>`;
  if (pending.length === 0) { $('wdapv_list').innerHTML = '<em>queue empty</em>'; return; }
  $('wdapv_list').innerHTML = `<table><thead><tr>
    <th>id</th><th>user</th><th>chain</th><th>asset</th><th class="num">amount</th><th>dest</th><th>status</th><th>approval req</th><th>submitted</th><th></th>
    </tr></thead><tbody>${
    pending.map(w => {
      const link = wdApvs.find(a => a.resource?.id === w.withdrawal_id);
      return `<tr>
        <td>${(w.withdrawal_id||'').slice(0,18)}</td>
        <td>${w.user_id}</td>
        <td>${w.chain}</td>
        <td>${w.asset || (CHAIN_ASSETS[w.chain]?.[0]?.symbol ?? '—')}</td>
        <td class="num">${(w.amount||0).toLocaleString()}</td>
        <td>${(w.destination_address||'').slice(0,18)}…</td>
        <td><span class="pill warn">${w.status}</span></td>
        <td>${link ? `<span class="pill">${link.approval_request_id?.slice(0,10)}</span>` : '<em style="color:var(--mute)">none</em>'}</td>
        <td>${(w.submitted_at||'').slice(0,19)}</td>
        <td><button onclick="document.getElementById('wdapv_id').value='${w.withdrawal_id}';scrollTo(0,9999)" style="padding:2px 8px;font-size:11px">act</button></td>
      </tr>`;
    }).join('')
  }</tbody></table>`;
}
export async function admWdApvAct(decision) {
  const wid = $('wdapv_id').value;
  if (!wid) { showResult('wdapv_act', '<em>provide withdrawal_id</em>'); return; }
  const r = await asAdmin(()=>get('/admin/approval-requests', { silent:true }))();
  const apvs = r.json?.pending || r.json?.requests || (Array.isArray(r.json)? r.json : []);
  const link = apvs.find(a => a.resource?.kind === 'withdrawal' && a.resource?.id === wid);
  if (!link) {
    showResult('wdapv_act', `<em>no approval-request found for withdrawal ${wid} — wd-MC linking is a follow-up.</em>`);
    return;
  }
  const path = `/admin/approval-requests/${encodeURIComponent(link.approval_request_id)}/${decision}`;
  const out = await call('POST', path, JSON.stringify({ reason: $('wdapv_reason').value }), { subject: $('wdapv_who').value, role: 'admin' });
  showResult('wdapv_act', `<dl class="kv"><dt>status</dt><dd>${out.status}</dd></dl>${renderJSON(out.json || out.text)}`);
  admWdApvLoad();
}

export function renderAdminAuditSearch(c) {
  c.innerHTML = `
    <h2>Audit Search <small>filter <code>/admin/audit/actions</code></small></h2>
    <div class="card">
      <div class="row">
        <label>limit</label>${input('as_limit','200')}
        <label>subject contains</label>${input('as_subj','')}
        <label>action contains</label>${input('as_action','')}
      </div>
      <div class="row">
        <label>role</label>${select('as_role',['(any)','user','admin','system'])}
        <label>since (UTC ISO)</label>${input('as_since','')}
        <label>until (UTC ISO)</label>${input('as_until','')}
      </div>
      <div class="row btns">
        <button class="primary" onclick="admAuditSearch()">search</button>
        <button onclick="admAuditExport()">export json</button>
        <small id="as_count" style="margin-left:auto;color:var(--mute)"></small>
      </div>
      <div id="as_table"></div>
    </div>`;
}
export async function admAuditSearch() {
  const r = await asAdmin(()=>get('/admin/audit/actions?limit=' + encodeURIComponent($('as_limit').value)))();
  let items = r.json?.items || r.json?.entries || r.json?.actions || [];
  const sf = $('as_subj').value.trim().toLowerCase();
  const af = $('as_action').value.trim().toLowerCase();
  const rf = $('as_role').value;
  const since = $('as_since').value.trim();
  const until = $('as_until').value.trim();
  items = items.filter(a => {
    const ts2 = a.recorded_at || a.timestamp || a.ts || '';
    if (sf && !(a.subject||'').toLowerCase().includes(sf)) return false;
    if (af && !(a.action||'').toLowerCase().includes(af)) return false;
    if (rf !== '(any)' && a.role !== rf) return false;
    if (since && ts2 < since) return false;
    if (until && ts2 > until) return false;
    return true;
  });
  $('as_count').textContent = `${items.length} matching`;
  window.__lastAuditSearch = items;
  $('as_table').innerHTML = items.length === 0 ? '<em>no rows match — try widening the filter</em>' : table(
    ['ts','subject','role','action','request_id','detail'],
    items.slice(0,500).map(a => [
      (a.recorded_at||a.timestamp||a.ts||'').slice(0,19),
      a.subject, a.role,
      `<code>${escapeHtml(a.action||'')}</code>`,
      (a.request_id||'').slice(0,12),
      `<details><summary style="color:var(--mute);cursor:pointer">show</summary><pre style="font-size:11px;background:#010409;padding:4px;border-radius:3px;max-width:600px;overflow:auto">${escapeHtml(JSON.stringify(a, null, 2))}</pre></details>`,
    ]));
}
export function admAuditExport() {
  const items = window.__lastAuditSearch || [];
  if (items.length === 0) { logErr('no search results to export — run search first'); return; }
  const blob = new Blob([JSON.stringify(items, null, 2)], { type:'application/json' });
  const a = document.createElement('a');
  a.href = URL.createObjectURL(blob);
  a.download = 'audit-' + new Date().toISOString().slice(0,19).replace(/[:T]/g,'') + '.json';
  document.body.appendChild(a); a.click(); a.remove();
}
