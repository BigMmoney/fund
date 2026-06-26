// js/router.js — page registry + sidebar + navigate.
import { $, modeAllowsApp, setBadge, clearPageTimers } from './core.js';
import { stopAllKlineStreams } from './charts.js';
import { stopUserTradeStreams } from './pages-user.js';
import * as user from './pages-user.js';
import * as admin from './pages-admin.js';
import * as ops from './pages-ops.js';
import * as comp from './pages-compliance.js';

export const PAGES = {
  user: {
    dashboard:     { title:'Dashboard',     render: user.renderUserDashboard },
    markets:       { title:'Markets',       render: user.renderUserMarkets },
    trade:         { title:'Trade',         render: user.renderUserTrade },
    orders:        { title:'Orders',        render: user.renderUserOrders },
    balances:      { title:'Balances',      render: user.renderUserBalances },
    deposit:       { title:'Deposit',       render: user.renderUserDeposit },
    withdraw:      { title:'Withdraw',      render: user.renderUserWithdraw },
    addresses:     { title:'Address Book',  render: user.renderUserAddresses },
    history:       { title:'History',       render: user.renderUserHistory },
    api_keys:      { title:'API Keys',      render: user.renderUserApiKeys },
    notifications: { title:'Notifications', render: user.renderUserNotifications },
    security:      { title:'Security',      render: user.renderUserSecurity },
  },
  admin: {
    overview:            { title:'Overview',            render: admin.renderAdminOverview },
    customers:           { title:'Customers',           render: admin.renderAdminCustomers },
    customer_detail:     { title:'Customer Detail',     render: admin.renderAdminCustomerDetail },
    queue:               { title:'Wallet Queue',        render: admin.renderAdminQueue },
    withdrawal_approval: { title:'Withdrawal Approval', render: admin.renderAdminWithdrawalApproval },
    addresses:           { title:'Addresses',           render: admin.renderAdminAddresses },
    approvals:           { title:'Approvals',           render: admin.renderAdminApprovals },
    employees:           { title:'Employees',           render: admin.renderAdminEmployees },
    grants:              { title:'Role Grants',         render: admin.renderAdminGrants },
    market_controls:     { title:'Market Controls',     render: admin.renderAdminMarketControls },
    transfers:           { title:'Internal Transfers',  render: admin.renderAdminTransfers },
    audit:               { title:'Audit Logs',          render: admin.renderAdminAudit },
    audit_search:        { title:'Audit Search',        render: admin.renderAdminAuditSearch },
  },
  ops: {
    health:    { title:'System Health',  render: ops.renderOpsHealth },
    chain:     { title:'Chain Health',   render: ops.renderOpsChain },
    hot:       { title:'Hot Wallets',    render: ops.renderOpsHotWallets },
    workers:   { title:'Worker Status',  render: ops.renderOpsWorkers },
    recon:     { title:'Reconciliation', render: ops.renderOpsRecon },
    backups:   { title:'Backups',        render: ops.renderOpsBackups },
    incidents: { title:'Incidents',      render: ops.renderOpsIncidents },
    kill:      { title:'Kill Switches',  render: ops.renderOpsKill },
    smoke:     { title:'Smoke Console',  render: ops.renderOpsSmoke },
  },
  compliance: {
    sanctions:        { title:'Sanctions',           render: comp.renderCompSanctions },
    sanctions_review: { title:'Sanctions Review',    render: comp.renderCompSanctionsReview },
    review:           { title:'Manual Review',       render: comp.renderCompReview },
    suspended:        { title:'Suspended Addresses', render: comp.renderCompSuspended },
    reports:           { title:'Reports',             render: comp.renderCompReports },
    retention:        { title:'Retention / Export', render: comp.renderCompRetention },
  },
};
// Expose so palette + page-internal references via window keep working.
window.PAGES = PAGES;

export function renderSidebar(app, page) {
  const items = Object.entries(PAGES[app]).map(([slug, { title }]) =>
    `<a class="nav ${slug===page?'active':''}" href="#${app}/${slug}">${title}</a>`
  ).join('');
  $('sidebar').innerHTML = `<div class="group">${app}</div>${items}`;
  $('appnav').querySelectorAll('a').forEach(a => a.classList.toggle('active', a.dataset.app === app));
}

export function navigate() {
  const h = (location.hash || '#user/dashboard').replace(/^#/,'');
  const [app, page] = h.split('/');
  if (!PAGES[app] || !PAGES[app][page]) { location.hash = '#user/dashboard'; return; }
  if (!modeAllowsApp(app)) { location.hash = '#user/dashboard'; return; }
  // Tear down any in-flight WS trade streams + page-scoped timers.
  stopAllKlineStreams();
  stopUserTradeStreams();
  clearPageTimers();
  renderSidebar(app, page);
  PAGES[app][page].render($('content'));
  if (window.__lastBadges) for (const [k, v] of Object.entries(window.__lastBadges)) {
    const [a, s] = k.split(':'); setBadge(a, s, v.count, v.level);
  }
}
