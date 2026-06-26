// js/pages-ops.js — Ops app page renderers.
import {
  $, escapeHtml, get, post, call, asAdmin, ts, logRaw,
  form, input, select, textarea, table, stub, renderJSON, showResult,
  DEFAULT_MARKET, CHAIN_ASSETS, marketSelect,
  toast, confirmModal, setPageInterval,
} from './core.js';

export function renderOpsHealth(c) {
  c.innerHTML = `<h2>System Health <small>/health detailed</small></h2><div class="card">${form([], `<button class="primary" onclick="opsHealthLoad()">refresh</button>`)}<div id="health_detail"></div></div>`;
  opsHealthLoad();
}
export async function opsHealthLoad() {
  const r = await get('/health');
  if (r.json) {
    const h = r.json;
    showResult('health_detail', `<dl class="kv">
      <dt>status</dt><dd><span class="pill ${h.status==='ok'?'ok':'err'}">${h.status}</span></dd>
      <dt>uptime</dt><dd>${h.uptime_secs}s</dd>
      <dt>accounts</dt><dd>${h.accounts}</dd>
      <dt>seen op_ids</dt><dd>${h.seen_op_ids}</dd>
      <dt>kill switch</dt><dd>${h.kill_switch}</dd>
      <dt>bridge alive</dt><dd>${h.bridge_alive}</dd>
      <dt>frontiers consistent</dt><dd>${h.frontiers?.consistent}</dd>
    </dl>${renderJSON(h)}`);
  }
}

export function renderOpsChain(c) {
  c.innerHTML = `
    <h2>Chain Health <small>per-chain RPC + reorg + balance vs ledger</small></h2>
    <div class="row btns" style="margin-bottom:10px">
      <button class="primary" onclick="opsChainLoad()">refresh all</button>
      <small style="margin-left:auto;color:var(--mute)">Sources: <code>/admin/risk/reconciliation/core-chain</code> + <code>/admin/wallet/balances</code> + <code>/admin/risk/reconciliation/settlements</code>.</small>
    </div>
    <div id="chain_stats" class="stats"></div>
    <div id="chain_per" class="split"></div>
    <div class="card"><h3>raw core-chain payload</h3><div id="chain_raw"></div></div>`;
  opsChainLoad();
}
export async function opsChainLoad() {
  const [core, hot, sets] = await Promise.all([
    asAdmin(()=>get('/admin/risk/reconciliation/core-chain', { silent:true }))(),
    asAdmin(()=>get('/admin/wallet/balances', { silent:true }))(),
    asAdmin(()=>get('/admin/risk/reconciliation/settlements', { silent:true }))(),
  ]);
  const chains = hot.json?.chains || [];
  const overall = core.json || {};
  const breaches = (overall.breaches || []).length;
  $('chain_stats').innerHTML = `
    <div class="stat ${breaches?'err':'ok'}"><div class="label">core-chain breaches</div><div class="val">${breaches}</div><div class="sub">INV-5 violations</div></div>
    <div class="stat"><div class="label">tracked chains</div><div class="val">${chains.length}</div><div class="sub">eth · btc · sol</div></div>
    <div class="stat ${overall.status==='ok'?'ok':'err'}"><div class="label">consistency</div><div class="val">${overall.status || '?'}</div><div class="sub">last check</div></div>`;
  $('chain_per').innerHTML = chains.length === 0 ? '<em>no chains in /admin/wallet/balances yet</em>' : chains.map(c => {
    const conf = { eth: 25, btc: 6, sol: 32 }[c.chain] || '—';
    const assets = (CHAIN_ASSETS[c.chain] || []).map(a=>a.symbol).join(' · ');
    return `<div class="card">
      <h3>${c.chain.toUpperCase()} <span style="color:var(--mute);font-size:11px">${assets}</span></h3>
      <dl class="kv">
        <dt>hot address</dt><dd style="font-family:ui-monospace,Menlo,Consolas,monospace;font-size:11px">${c.hot_address || '—'}</dd>
        <dt>hot balance</dt><dd>${(c.hot_balance ?? 0).toLocaleString()}</dd>
        <dt>outstanding</dt><dd>${c.outstanding_count} req · ${(c.outstanding_reservations ?? 0).toLocaleString()} amount</dd>
        <dt>required confirmations</dt><dd>${conf}</dd>
        <dt>settlement account</dt><dd><code>SYS:WALLET:HOT:${c.chain}</code></dd>
      </dl>
    </div>`;
  }).join('');
  $('chain_raw').innerHTML = renderJSON({ core_chain: core.json, settlements: sets.json });
}

export function renderOpsHotWallets(c) {
  c.innerHTML = `<h2>Hot Wallets <small>/admin/wallet/balances</small></h2><div class="card">${form([], `<button class="primary" onclick="opsHotLoad()">refresh</button>`)}<div id="hot_out"></div></div>`;
  opsHotLoad();
}
export async function opsHotLoad() {
  const r = await asAdmin(()=>get('/admin/wallet/balances'))();
  const chains = r.json?.chains || [];
  showResult('hot_out', chains.length === 0 ? renderJSON(r.json) : table(
    ['chain','hot address','hot balance','outstanding (count)','outstanding (amount)'],
    chains.map(c => [c.chain, c.hot_address, c.hot_balance, c.outstanding_count, c.outstanding_reservations])));
}

export function renderOpsWorkers(c) {
  c.innerHTML = `
    <h2>Worker Status <small>capacity + per-worker liveness</small></h2>
    <div class="row btns" style="margin-bottom:10px">
      <button class="primary" onclick="opsWorkLoad()">refresh</button>
      <label style="margin-left:14px">auto</label>
      <select id="ws_auto"><option value="0">off</option><option value="3">3s</option><option value="10">10s</option></select>
      <small id="ws_when" style="margin-left:auto;color:var(--mute)"></small>
    </div>
    <div id="ws_stats" class="stats"></div>
    <div class="card"><h3>capacity dimensions</h3><div id="work_out"></div></div>
    <div class="card"><h3>per-worker (synthetic — composed from frontiers + queue depths)</h3><div id="ws_workers"></div></div>
    ${stub('Per-worker liveness derived from <code>/health.frontiers</code> + <code>/admin/wallet/queue</code>.', '/admin/workers')}`;
  opsWorkLoad();
  $('ws_auto').addEventListener('change', () => {
    if (window.__wsTimer) { clearInterval(window.__wsTimer); window.__wsTimer = null; }
    const v = parseInt($('ws_auto').value, 10);
    if (v > 0) {
      window.__wsTimer = setPageInterval(() => {
        if (!document.getElementById('ws_auto')) return;
        opsWorkLoad();
      }, v*1000);
    }
  });
}
export async function opsWorkLoad() {
  const [cap, h, q] = await Promise.all([
    asAdmin(()=>get('/admin/capacity', { silent:true }))(),
    get('/health', { silent:true }),
    asAdmin(()=>get('/admin/wallet/queue', { silent:true }))(),
  ]);
  const c = cap.json?.capacity;
  if (c) {
    const meas = c.measurements || [];
    const rows = meas.map(m => {
      const pct = m.utilization_pct || 0;
      const cls = pct > 90 ? 'err' : pct > 70 ? 'warn' : '';
      return [m.dimension, m.current, m.limit, `<span class="pill ${cls}">${pct.toFixed(1)}%</span>`, m.status];
    });
    const alerts = (c.alerts||[]).length;
    $('work_out').innerHTML = `<dl class="kv"><dt>collected</dt><dd>${(c.collected_at||'').slice(11,19)}</dd><dt>active alerts</dt><dd>${alerts}</dd></dl>` +
      table(['dimension','current','limit','util','status'], rows);
    const breaching = meas.filter(m => (m.utilization_pct||0) > 90).length;
    const warn = meas.filter(m => { const p=m.utilization_pct||0; return p > 70 && p <= 90; }).length;
    $('ws_stats').innerHTML = `
      <div class="stat ${breaching?'err':warn?'warn':'ok'}"><div class="label">capacity</div><div class="val">${meas.length}</div><div class="sub">${breaching} breaching · ${warn} warning</div></div>
      <div class="stat ${alerts?'err':'ok'}"><div class="label">alerts</div><div class="val">${alerts}</div><div class="sub">active capacity alerts</div></div>
      <div class="stat ${h.json?.bridge_alive?'ok':'err'}"><div class="label">bridge</div><div class="val">${h.json?.bridge_alive?'alive':'down'}</div><div class="sub">sequencer ↔ engine</div></div>
      <div class="stat ${h.json?.frontiers?.consistent?'ok':'err'}"><div class="label">frontiers</div><div class="val">${h.json?.frontiers?.consistent?'consistent':'drifted'}</div><div class="sub">cmd_seq alignment</div></div>`;
  }
  const fr = h.json?.frontiers || {};
  const pending = (q.json?.pending || []).length;
  const stuck = (q.json?.pending || []).filter(w => w.status === 'settlement_stuck').length;
  $('ws_workers').innerHTML = table(
    ['worker','observed seq / depth','status','source'],
    [
      ['sequencer', fr.sequencer_command_seq ?? '—', `<span class="pill ${fr.consistent?'ok':'err'}">${fr.consistent?'live':'drift'}</span>`, '/health.frontiers'],
      ['ledger', fr.ledger_command_seq ?? '—', `<span class="pill ${fr.consistent?'ok':'err'}">${fr.consistent?'live':'drift'}</span>`, '/health.frontiers'],
      ['order projection', fr.order_projection_command_seq ?? '—', `<span class="pill ${fr.consistent?'ok':'err'}">${fr.consistent?'live':'drift'}</span>`, '/health.frontiers'],
      ['trade log', fr.trade_log_command_seq ?? '—', `<span class="pill">idle</span>`, '/health.frontiers'],
      ['trade settlement', fr.trade_settlement_command_seq ?? '—', `<span class="pill">idle</span>`, '/health.frontiers'],
      ['hot-wallet daemon', pending + ' pending wd', `<span class="pill ${stuck?'err':'ok'}">${stuck?stuck+' stuck':'green'}</span>`, '/admin/wallet/queue'],
      ['settlement worker', (q.json?.pending || []).filter(w=>w.status==='confirmed').length + ' confirmed→settled', `<span class="pill ok">draining</span>`, '/admin/wallet/queue'],
    ]);
  $('ws_when').textContent = 'updated ' + new Date().toISOString().slice(11,19);
}

export function renderOpsRecon(c) {
  c.innerHTML = `
    <h2>Reconciliation <small>invariants INV-1 / INV-3 / INV-4 / INV-5</small></h2>
    <div class="row btns" style="margin-bottom:10px"><button class="primary" onclick="opsReconLoad()">refresh all</button></div>
    <div id="recon_stats" class="stats"></div>
    <div class="card"><h3>invariants snapshot</h3><div id="recon_invariants"></div></div>
    <div class="split">
      <div class="card"><h3>settlements report <small>/admin/risk/reconciliation/settlements</small></h3><div id="recon_set"></div></div>
      <div class="card"><h3>core-chain report <small>/admin/risk/reconciliation/core-chain</small></h3><div id="recon_core"></div></div>
    </div>
    <div class="card"><h3>drill</h3><div class="desc">Run from a terminal: <code>scripts/reconcile_drill.ps1</code>.</div></div>
    <div class="card"><h3>per-chain coverage</h3><div class="desc">INV-5 holds per chain in the ChainSpec set: <strong>eth</strong>, <strong>btc</strong>, <strong>sol</strong>.</div></div>`;
  opsReconLoad();
}
export async function opsReconLoad() {
  const [s, c] = await Promise.all([
    asAdmin(()=>get('/admin/risk/reconciliation/settlements', { silent:true }))(),
    asAdmin(()=>get('/admin/risk/reconciliation/core-chain', { silent:true }))(),
  ]);
  const sj = s.json || {}; const cj = c.json || {};
  const breaches = (sj.breaches?.length || 0) + (cj.breaches?.length || 0);
  const inv1 = sj.inv1 || sj.ledger_sum_zero || (sj.status === 'ok' ? { status:'ok' } : { status:'unknown' });
  const inv3 = sj.inv3 || sj.no_duplicate_op_ids || { status: sj.status || 'unknown' };
  const inv4 = sj.inv4 || sj.settled_match || { status: sj.status || 'unknown' };
  const inv5 = cj.inv5 || cj.chain_match || { status: cj.status || 'unknown' };
  $('recon_stats').innerHTML = `
    <div class="stat ${breaches?'err':'ok'}"><div class="label">breaches</div><div class="val">${breaches}</div><div class="sub">across all reports</div></div>
    <div class="stat ${(sj.status==='ok')?'ok':'warn'}"><div class="label">settlements</div><div class="val">${sj.status||'?'}</div><div class="sub">${sj.checked_at?.slice(11,19) || '—'}</div></div>
    <div class="stat ${(cj.status==='ok')?'ok':'warn'}"><div class="label">core-chain</div><div class="val">${cj.status||'?'}</div><div class="sub">${cj.checked_at?.slice(11,19) || '—'}</div></div>
    <div class="stat"><div class="label">last drill</div><div class="val">manual</div><div class="sub">scripts/reconcile_drill.ps1</div></div>`;
  const pillFor = (st) => `<span class="pill ${st==='ok'?'ok':st==='unknown'?'':'err'}">${st || '?'}</span>`;
  $('recon_invariants').innerHTML = table(
    ['invariant','rule','status','source'],
    [
      ['INV-1','Σ ledger entries = 0', pillFor(inv1.status), '/admin/risk/reconciliation/settlements'],
      ['INV-3','no duplicate op_id', pillFor(inv3.status), '/admin/risk/reconciliation/settlements'],
      ['INV-4','Settled withdrawal ↔ wd-settle entry', pillFor(inv4.status), '/admin/risk/reconciliation/settlements'],
      ['INV-5','per-chain on-chain Σ ↔ ledger Σ', pillFor(inv5.status), '/admin/risk/reconciliation/core-chain'],
    ]);
  $('recon_set').innerHTML = sj.status ? renderJSON(sj) : '<em>no payload</em>';
  $('recon_core').innerHTML = cj.status ? renderJSON(cj) : '<em>no payload</em>';
}

export function renderOpsBackups(c) {
  c.innerHTML = `
    <h2>Backups <small>5-min off-host snapshots</small></h2>
    ${stub('No live status endpoint — backup runs as systemd timer / k8s cronjob on a 5-min cadence.', '/admin/backups/status')}
    <div id="bk_stats" class="stats">
      <div class="stat ok"><div class="label">cadence</div><div class="val" style="font-size:18px">5 min</div><div class="sub">systemd timer / k8s cronjob</div></div>
      <div class="stat ok"><div class="label">retention</div><div class="val" style="font-size:18px">90 days</div><div class="sub">on the bucket lifecycle</div></div>
      <div class="stat"><div class="label">target bucket</div><div class="val" style="font-size:14px">$BACKUP_BUCKET</div><div class="sub">env-configured</div></div>
      <div class="stat"><div class="label">latest pointer</div><div class="val" style="font-size:14px">LATEST</div><div class="sub">s3://$BACKUP_BUCKET/LATEST</div></div>
    </div>
    <div class="split">
      <div class="card"><h3>cadence script</h3>
        <pre style="font-size:12px;background:#010409;padding:10px;border-radius:3px;border:1px solid var(--border)">OnUnitActiveSec=5min
trap "rm -rf $workdir" EXIT
tar -czf data.tar.gz -C $workdir data
sha256sum data > MANIFEST.sha256
aws s3 cp data.tar.gz   s3://$BACKUP_BUCKET/$ts/
aws s3 cp MANIFEST.sha  s3://$BACKUP_BUCKET/$ts/
echo $ts | aws s3 cp - s3://$BACKUP_BUCKET/LATEST</pre>
      </div>
      <div class="card"><h3>recent runs (synthetic)</h3><div id="bk_recent"></div></div>
    </div>
    <div class="card">
      <h3>restore drill</h3>
      <pre style="font-size:12px;background:#010409;padding:10px;border-radius:3px;border:1px solid var(--border)"># 1. fetch latest pointer
ts=$(aws s3 cp s3://$BACKUP_BUCKET/LATEST -)
# 2. download bundle + manifest
aws s3 cp s3://$BACKUP_BUCKET/$ts/data.tar.gz .
aws s3 cp s3://$BACKUP_BUCKET/$ts/MANIFEST.sha256 .
# 3. verify integrity
sha256sum -c MANIFEST.sha256
# 4. extract + boot a parallel exchange against the snapshot
tar -xzf data.tar.gz
DATA_DIR=./data ./target/release/api</pre>
      <div class="row btns">
        <button onclick="opsBackupVerifyStub()">simulate verify</button>
        <button onclick="opsBackupListStub()">simulate list snapshots</button>
      </div>
    </div>`;
  opsBackupRecentStub();
}
export function opsBackupRecentStub() {
  const now = Date.now();
  const slot = (n) => new Date(now - n*5*60_000 - (now % (5*60_000))).toISOString().slice(0,19).replace('T',' ');
  const rows = Array.from({length:12}, (_, i) => [slot(i), 'ok', 'tar+sha256 uploaded', `s3://$BACKUP_BUCKET/${slot(i).replace(/[-: ]/g,'')}/`]);
  $('bk_recent').innerHTML = table(['snapshot ts (UTC)','status','detail','s3 prefix'], rows);
}
export function opsBackupVerifyStub() {
  logRaw(`<span class="ts">${ts()}</span> <span class="ok">[backup] sha256sum -c MANIFEST.sha256 — would verify against latest snapshot bundle.</span>`);
}
export function opsBackupListStub() {
  logRaw(`<span class="ts">${ts()}</span> <span class="ok">[backup] aws s3 ls s3://$BACKUP_BUCKET/ — would list recent snapshot prefixes.</span>`);
}

export function renderOpsIncidents(c) {
  c.innerHTML = `
    <h2>Incidents <small>oncall + escalation</small></h2>
    <div class="card"><h3>oncall status</h3><div id="oncall_out"></div></div>
    <div class="card"><h3>escalation</h3><div id="escal_out"></div></div>
    <div class="card"><h3>runbooks</h3><div id="rb_out"></div></div>`;
  (async () => {
    const a = await asAdmin(()=>get('/admin/oncall/status', {silent:true}))();
    showResult('oncall_out', a.json ? renderJSON(a.json) : `<em>${a.status}</em>`);
    const b = await asAdmin(()=>get('/admin/oncall/escalation', {silent:true}))();
    showResult('escal_out', b.json ? renderJSON(b.json) : `<em>${b.status}</em>`);
    const c2 = await asAdmin(()=>get('/admin/oncall/runbooks', {silent:true}))();
    showResult('rb_out', c2.json ? renderJSON(c2.json) : `<em>${c2.status}</em>`);
  })();
}

export function renderOpsKill(c) {
  c.innerHTML = `
    <h2>Kill Switches <small>global + per-market</small></h2>
    <div class="card">
      <h3>global kill switch</h3>
      ${form([
        ['engaged', select('ks_eng',['true','false'])],
        ['reason (≥16)', input('ks_reason','operational drill from console')],
      ], `<button class="warn" onclick="opsKillSet()">POST /admin/kill-switch</button> <button onclick="opsKillGet()">GET</button>`)}
      <div id="ks_out"></div>
    </div>
    <div class="card">
      <h3>per-market state</h3>
      ${form([
        ['market', marketSelect('ms_market', DEFAULT_MARKET)],
        ['state', select('ms_state',['Open','HaltedTrading','HaltedAll','Closed'])],
        ['reason (≥16)', input('ms_reason','market state change drill from console')],
      ], `<button class="warn" onclick="opsMktSet()">POST /admin/market-state</button> <button onclick="opsMktGet()">GET</button>`)}
      <div id="ms_out"></div>
    </div>`;
}
export async function opsKillSet() {
  const engaged = $('ks_eng').value === 'true';
  const reason = $('ks_reason').value;
  if ((reason||'').length < 16) { toast('Reason must be ≥ 16 chars', 'warn'); return; }
  const ok = await confirmModal({
    title: engaged ? 'Engage GLOBAL kill switch?' : 'Release global kill switch?',
    body: engaged
      ? 'This stops <strong>all</strong> matching across all markets.'
      : 'Release the global kill switch and resume matching across all markets.',
    okLabel: engaged ? 'Engage' : 'Release', danger: engaged,
  });
  if (!ok) return;
  const r = await asAdmin(()=>post('/admin/kill-switch', { engaged, reason }))();
  toast(r.status === 200 ? (engaged ? 'Kill switch engaged' : 'Kill switch released') : 'Action failed', r.status === 200 ? (engaged ? 'warn' : 'ok') : 'err');
  opsKillGet();
}
export async function opsKillGet() {
  const r = await get('/health', { silent:true });
  showResult('ks_out', r.json ? `<dl class="kv"><dt>kill_switch</dt><dd><span class="pill ${r.json.kill_switch?'err':'ok'}">${r.json.kill_switch ? 'engaged' : 'normal'}</span></dd></dl>` : '<em>health probe failed</em>');
}
export async function opsMktSet() {
  const body = { market: $('ms_market').value, state: $('ms_state').value, reason: $('ms_reason').value };
  await asAdmin(()=>post('/admin/market-state', body))();
}
export async function opsMktGet() {
  const r = await asAdmin(()=>get('/admin/market-state/' + encodeURIComponent($('ms_market').value)))();
  showResult('ms_out', r.json ? renderJSON(r.json) : '');
}

export function renderOpsSmoke(c) {
  c.innerHTML = `
    <h2>Smoke Console <small>arbitrary signed request</small></h2>
    <div class="card">
      ${form([
        ['method', select('sm_method',['GET','POST','DELETE','PUT','PATCH'])],
        ['path + query', input('sm_path','/admin/wallet/queue')],
        ['body (json)', textarea('sm_body','')],
        ['as subject', input('sm_subject','admin-test')],
        ['as role', select('sm_role',['admin','user'])],
      ], `<button class="primary" onclick="opsSmokeSend()">send</button>`)}
      <div id="sm_out"></div>
    </div>`;
}
export async function opsSmokeSend() {
  const body = $('sm_body').value.trim();
  const r = await call($('sm_method').value, $('sm_path').value, body, { subject: $('sm_subject').value, role: $('sm_role').value });
  showResult('sm_out', `<dl class="kv"><dt>status</dt><dd>${r.status}</dd></dl>${renderJSON(r.json || r.text)}`);
}
