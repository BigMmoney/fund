// js/main.js — entry point. Imports modules, wires inline-handler globals
// onto window (so onclick="userOrderSubmit()" still resolves), and runs startup.

import * as core from './core.js';
import * as charts from './charts.js';
import * as binance from './binance.js';
import * as user from './pages-user.js';
import * as admin from './pages-admin.js';
import * as ops from './pages-ops.js';
import * as comp from './pages-compliance.js';
import * as router from './router.js';

// Attach all named exports to window so the inline event handlers
// scattered across the rendered HTML fragments continue to resolve.
// Use an explicit loop rather than Object.assign(window, moduleNS) — the latter
// can be flaky across module-namespace exotic objects in some browser builds.
for (const mod of [core, charts, binance, user, admin, ops, comp, router]) {
  for (const k in mod) {
    try { window[k] = mod[k]; } catch {}
  }
}
window.__moduleLoaded = true;

// One-off setup: base URL prefilled to current origin.
core.$('base').value = location.origin;

// Static-button wiring (replaces inline onclicks that race the module load).
const onClick = (id, fn) => { const el = core.$(id); if (el) el.addEventListener('click', fn); };
onClick('btn_palette_who', core.openPalette);
onClick('btn_palette_id',  core.openPalette);
onClick('btn_clear_log',   core.clearLog);
onClick('btn_toggle_log',  core.toggleLogPane);

// Mode toggle wiring + nav listeners.
window.addEventListener('hashchange', router.navigate);
core.$('appnav').addEventListener('click', e => {
  if (e.target.dataset.app) {
    e.preventDefault();
    const app = e.target.dataset.app;
    const first = Object.keys(router.PAGES[app])[0];
    location.hash = `#${app}/${first}`;
  }
});

document.querySelectorAll('#mode_toggle a, #mode_toggle_who a').forEach(a => a.addEventListener('click', () => {
  const prevHash = location.hash;
  core.setMode(a.dataset.mode);
  if (location.hash === prevHash) router.navigate();
}));

// Cmd/Ctrl+K opens the palette.
document.addEventListener('keydown', (e) => {
  if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
    e.preventDefault();
    core.openPalette();
  }
});

// Bootstrap: persist identity, set view mode, navigate, start health poll.
(async () => {
  core.persistIdentity();
  core.setMode(localStorage.getItem('uiMode') || 'user');
  try {
    const r = await fetch(core.$('base').value + '/health');
    const elH = core.$('health');
    if (r.ok) { elH.textContent = 'up'; elH.classList.add('ok'); }
    else      { elH.textContent = 'http ' + r.status; elH.classList.add('err'); }
  } catch { core.$('health').textContent = 'down'; core.$('health').classList.add('err'); }
  router.navigate();
  core.refreshHealthAndBadges();
  setInterval(core.refreshHealthAndBadges, 15_000);
})();
