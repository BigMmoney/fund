//! WebSocket real-time feed for trades, orderbook snapshots, and user fills.
//!
//! Routes:
//!   GET /ws/trades/:market_id      — public trade stream
//!   GET /ws/orderbook/:market_id   — periodic orderbook snapshot pushes
//!
//! Each connected client receives JSON frames:
//! ```json
//! { "type": "trade", "market_id": "BTC-USD", "data": { ... } }
//! ```

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use futures_util::{SinkExt, StreamExt};
use parking_lot::RwLock;
use tokio::sync::broadcast;
use warp::filters::BoxedFilter;
use warp::ws::{Message, WebSocket};
use warp::{Filter, Reply};

use super::observability;
use super::security::{require_user, with_principal};
use types::AuthenticatedPrincipal;

/// Default maximum concurrent WebSocket connections (overridden by config).
const DEFAULT_MAX_CONNECTIONS: usize = 1024;

/// Default maximum concurrent WebSocket connections per single IP address.
const DEFAULT_MAX_CONNECTIONS_PER_IP: usize = 20;

/// Feed event pushed to subscribers.
#[derive(Debug, Clone, serde::Serialize)]
pub struct WsFeedEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub market_id: String,
    pub data: serde_json::Value,
}

/// Shared hub that fans out events to all WebSocket subscribers per market.
#[derive(Clone)]
pub struct WsHub {
    /// market_id → broadcast sender (trades)
    trade_channels: Arc<RwLock<HashMap<String, broadcast::Sender<WsFeedEvent>>>>,
    /// market_id → broadcast sender (orderbook snapshots)
    orderbook_channels: Arc<RwLock<HashMap<String, broadcast::Sender<WsFeedEvent>>>>,
    /// user_id → broadcast sender (private fills, balance changes)
    user_channels: Arc<RwLock<HashMap<String, broadcast::Sender<WsFeedEvent>>>>,
    /// market_id → broadcast sender (ticker updates)
    ticker_channels: Arc<RwLock<HashMap<String, broadcast::Sender<WsFeedEvent>>>>,
    /// market_id → broadcast sender (mark price updates)
    mark_price_channels: Arc<RwLock<HashMap<String, broadcast::Sender<WsFeedEvent>>>>,
    /// global broadcast sender (liquidation events)
    liquidation_tx: Arc<broadcast::Sender<WsFeedEvent>>,
    connection_count: Arc<std::sync::atomic::AtomicUsize>,
    max_connections: usize,
    /// Per-IP connection counts for DoS mitigation.
    connections_per_ip: Arc<DashMap<IpAddr, usize>>,
    max_connections_per_ip: usize,
    /// Shutdown signal: when sent, all WS loops should exit.
    shutdown_tx: Arc<broadcast::Sender<()>>,
}

impl WsHub {
    pub fn new() -> Self {
        Self::with_max_connections(DEFAULT_MAX_CONNECTIONS)
    }

    pub fn with_max_connections(max: usize) -> Self {
        Self::with_limits(max, DEFAULT_MAX_CONNECTIONS_PER_IP)
    }

    pub fn with_limits(max: usize, max_per_ip: usize) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        let (liquidation_tx, _) = broadcast::channel(128);
        Self {
            trade_channels: Arc::new(RwLock::new(HashMap::new())),
            orderbook_channels: Arc::new(RwLock::new(HashMap::new())),
            user_channels: Arc::new(RwLock::new(HashMap::new())),
            ticker_channels: Arc::new(RwLock::new(HashMap::new())),
            mark_price_channels: Arc::new(RwLock::new(HashMap::new())),
            liquidation_tx: Arc::new(liquidation_tx),
            connection_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            max_connections: max,
            connections_per_ip: Arc::new(DashMap::new()),
            max_connections_per_ip: max_per_ip,
            shutdown_tx: Arc::new(shutdown_tx),
        }
    }

    /// Current number of active WS connections.
    pub fn connection_count(&self) -> usize {
        self.connection_count
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Signal all WS connection loops to shut down gracefully.
    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(());
    }

    /// Subscribe to the shutdown signal.
    fn subscribe_shutdown(&self) -> broadcast::Receiver<()> {
        self.shutdown_tx.subscribe()
    }

    /// Publish a trade event to all subscribers of a given market.
    pub fn publish_trade(&self, market_id: &str, data: serde_json::Value) {
        let channels = self.trade_channels.read();
        if let Some(sender) = channels.get(market_id) {
            let _ = sender.send(WsFeedEvent {
                event_type: "trade".into(),
                market_id: market_id.to_string(),
                data,
            });
        }
    }

    /// Publish an orderbook snapshot to all subscribers of a given market.
    pub fn publish_orderbook(&self, market_id: &str, data: serde_json::Value) {
        let channels = self.orderbook_channels.read();
        if let Some(sender) = channels.get(market_id) {
            let _ = sender.send(WsFeedEvent {
                event_type: "orderbook".into(),
                market_id: market_id.to_string(),
                data,
            });
        }
    }

    /// Subscribe to a market's trade feed. Creates the channel if absent.
    fn subscribe_trades(&self, market_id: &str) -> broadcast::Receiver<WsFeedEvent> {
        let mut channels = self.trade_channels.write();
        let sender = channels
            .entry(market_id.to_string())
            .or_insert_with(|| broadcast::channel(256).0);
        sender.subscribe()
    }

    /// Subscribe to a market's orderbook feed. Creates the channel if absent.
    fn subscribe_orderbook(&self, market_id: &str) -> broadcast::Receiver<WsFeedEvent> {
        let mut channels = self.orderbook_channels.write();
        let sender = channels
            .entry(market_id.to_string())
            .or_insert_with(|| broadcast::channel(64).0);
        sender.subscribe()
    }

    /// Publish a private event to a specific user.
    pub fn publish_user_event(&self, user_id: &str, event: WsFeedEvent) {
        let channels = self.user_channels.read();
        if let Some(sender) = channels.get(user_id) {
            let _ = sender.send(event);
        }
    }

    /// Subscribe to a user's private feed. Creates the channel if absent.
    fn subscribe_user(&self, user_id: &str) -> broadcast::Receiver<WsFeedEvent> {
        let mut channels = self.user_channels.write();
        let sender = channels
            .entry(user_id.to_string())
            .or_insert_with(|| broadcast::channel(128).0);
        sender.subscribe()
    }

    /// Publish a ticker update to all subscribers of a given market.
    pub fn publish_ticker(&self, market_id: &str, data: serde_json::Value) {
        let channels = self.ticker_channels.read();
        if let Some(sender) = channels.get(market_id) {
            let _ = sender.send(WsFeedEvent {
                event_type: "ticker".into(),
                market_id: market_id.to_string(),
                data,
            });
        }
    }

    /// Subscribe to a market's ticker feed. Creates the channel if absent.
    fn subscribe_ticker(&self, market_id: &str) -> broadcast::Receiver<WsFeedEvent> {
        let mut channels = self.ticker_channels.write();
        let sender = channels
            .entry(market_id.to_string())
            .or_insert_with(|| broadcast::channel(128).0);
        sender.subscribe()
    }

    /// Publish a mark price update to all subscribers of a given market.
    pub fn publish_mark_price(&self, market_id: &str, data: serde_json::Value) {
        let channels = self.mark_price_channels.read();
        if let Some(sender) = channels.get(market_id) {
            let _ = sender.send(WsFeedEvent {
                event_type: "mark_price".into(),
                market_id: market_id.to_string(),
                data,
            });
        }
    }

    /// Subscribe to a market's mark price feed.
    fn subscribe_mark_price(&self, market_id: &str) -> broadcast::Receiver<WsFeedEvent> {
        let mut channels = self.mark_price_channels.write();
        let sender = channels
            .entry(market_id.to_string())
            .or_insert_with(|| broadcast::channel(128).0);
        sender.subscribe()
    }

    /// Publish a liquidation event to all subscribers.
    pub fn publish_liquidation(&self, market_id: &str, data: serde_json::Value) {
        let _ = self.liquidation_tx.send(WsFeedEvent {
            event_type: "liquidation".into(),
            market_id: market_id.to_string(),
            data,
        });
    }

    /// Subscribe to the global liquidation feed.
    fn subscribe_liquidations(&self) -> broadcast::Receiver<WsFeedEvent> {
        self.liquidation_tx.subscribe()
    }

    fn add_connection(&self, remote_ip: IpAddr) -> bool {
        // Per-IP check first (DoS mitigation)
        {
            let mut ip_count = self.connections_per_ip.entry(remote_ip).or_insert(0);
            if *ip_count >= self.max_connections_per_ip {
                tracing::warn!(%remote_ip, "WS connection rejected: per-IP limit reached");
                return false;
            }
            *ip_count += 1;
        }

        // Global check
        let prev = self
            .connection_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if prev >= self.max_connections {
            self.connection_count
                .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
            // Rollback per-IP count
            if let Some(mut entry) = self.connections_per_ip.get_mut(&remote_ip) {
                *entry = entry.saturating_sub(1);
            }
            return false;
        }
        observability::METRICS
            .ws_connections_active
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        observability::METRICS
            .ws_connections_total
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        true
    }

    fn remove_connection(&self, remote_ip: IpAddr) {
        self.connection_count
            .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        observability::METRICS
            .ws_connections_active
            .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);

        // Decrement per-IP count, remove entry if zero
        if let Some(mut entry) = self.connections_per_ip.get_mut(&remote_ip) {
            *entry = entry.saturating_sub(1);
            if *entry == 0 {
                drop(entry);
                self.connections_per_ip.remove(&remote_ip);
            }
        }
    }
}

impl Default for WsHub {
    fn default() -> Self {
        Self::new()
    }
}

/// Sender half used to signal the WS message loops to track outgoing messages.
fn send_and_track(
    ws_tx: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    msg: Message,
) -> impl std::future::Future<Output = Result<(), warp::Error>> + '_ {
    observability::METRICS
        .ws_messages_sent
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    ws_tx.send(msg)
}

/// Build WebSocket warp routes.
pub fn build_ws_routes(hub: Arc<WsHub>) -> BoxedFilter<(warp::reply::Response,)> {
    let trades = {
        let hub = hub.clone();
        warp::path!("ws" / "trades" / String)
            .and(warp::ws())
            .and(warp::addr::remote())
            .map(
                move |market_id: String, ws: warp::ws::Ws, remote: Option<std::net::SocketAddr>| -> warp::reply::Response {
                    let hub = hub.clone();
                    let ip = remote
                        .map(|s| s.ip())
                        .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
                    ws.on_upgrade(move |socket| handle_trade_ws(socket, market_id, ip, hub))
                        .into_response()
                },
            )
            .boxed()
    };

    let orderbook = {
        let hub = hub.clone();
        warp::path!("ws" / "orderbook" / String)
            .and(warp::ws())
            .and(warp::addr::remote())
            .map(
                move |market_id: String, ws: warp::ws::Ws, remote: Option<std::net::SocketAddr>| -> warp::reply::Response {
                    let hub = hub.clone();
                    let ip = remote
                        .map(|s| s.ip())
                        .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
                    ws.on_upgrade(move |socket| handle_orderbook_ws(socket, market_id, ip, hub))
                        .into_response()
                },
            )
            .boxed()
    };

    let user = {
        let hub = hub.clone();
        warp::path!("ws" / "user")
            .and(with_principal())
            .and(warp::ws())
            .and(warp::addr::remote())
            .and_then(
                move |principal: AuthenticatedPrincipal,
                      ws: warp::ws::Ws,
                      remote: Option<std::net::SocketAddr>| {
                    let hub = hub.clone();
                    async move {
                        require_user(&principal)?;
                        let user_id = principal.subject.clone();
                        let ip = remote
                            .map(|s| s.ip())
                            .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
                        Ok::<warp::reply::Response, warp::Rejection>(
                            ws.on_upgrade(move |socket| handle_user_ws(socket, user_id, ip, hub))
                                .into_response(),
                        )
                    }
                },
            )
            .boxed()
    };

    let ticker = {
        let hub = hub.clone();
        warp::path!("ws" / "ticker" / String)
            .and(warp::ws())
            .and(warp::addr::remote())
            .map(
                move |market_id: String, ws: warp::ws::Ws, remote: Option<std::net::SocketAddr>| -> warp::reply::Response {
                    let hub = hub.clone();
                    let ip = remote
                        .map(|s| s.ip())
                        .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
                    ws.on_upgrade(move |socket| handle_ticker_ws(socket, market_id, ip, hub))
                        .into_response()
                },
            )
            .boxed()
    };

    let liquidations = {
        let hub = hub.clone();
        warp::path!("ws" / "liquidations")
            .and(warp::ws())
            .and(warp::addr::remote())
            .map(
                move |ws: warp::ws::Ws, remote: Option<std::net::SocketAddr>| -> warp::reply::Response {
                    let hub = hub.clone();
                    let ip = remote
                        .map(|s| s.ip())
                        .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
                    ws.on_upgrade(move |socket| handle_liquidation_ws(socket, ip, hub))
                        .into_response()
                },
            )
            .boxed()
    };

    let mark_price = {
        let hub = hub.clone();
        warp::path!("ws" / "mark-price" / String)
            .and(warp::ws())
            .and(warp::addr::remote())
            .map(
                move |market_id: String, ws: warp::ws::Ws, remote: Option<std::net::SocketAddr>| -> warp::reply::Response {
                    let hub = hub.clone();
                    let ip = remote
                        .map(|s| s.ip())
                        .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
                    ws.on_upgrade(move |socket| handle_mark_price_ws(socket, market_id, ip, hub))
                        .into_response()
                },
            )
            .boxed()
    };

    trades
        .or(orderbook)
        .unify()
        .or(user)
        .unify()
        .or(ticker)
        .unify()
        .or(liquidations)
        .unify()
        .or(mark_price)
        .unify()
        .boxed()
}

async fn handle_trade_ws(ws: WebSocket, market_id: String, remote_ip: IpAddr, hub: Arc<WsHub>) {
    if !hub.add_connection(remote_ip) {
        tracing::warn!("WS connection rejected: max connections reached");
        let (mut tx, _) = ws.split();
        let _ = tx
            .send(Message::close_with(1013u16, "max connections"))
            .await;
        return;
    }

    let mut rx = hub.subscribe_trades(&market_id);
    let mut shutdown_rx = hub.subscribe_shutdown();
    let (mut ws_tx, mut ws_rx) = ws.split();

    tracing::info!(market_id = %market_id, "WS trade feed connected");

    // Spawn ping task to keep connection alive.
    let ping_interval = Duration::from_secs(30);
    let (close_tx, mut close_rx) = tokio::sync::oneshot::channel::<()>();

    let ping_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(ping_interval);
        loop {
            tokio::select! {
                _ = interval.tick() => {}
                _ = &mut close_rx => break,
            }
        }
    });

    // Track consecutive send failures for backpressure.
    let mut consecutive_failures: u32 = 0;
    const MAX_SEND_FAILURES: u32 = 5;

    loop {
        tokio::select! {
            // Forward broadcast events to the WS client.
            result = rx.recv() => {
                match result {
                    Ok(event) => {
                        if let Ok(json) = serde_json::to_string(&event) {
                            if send_and_track(&mut ws_tx, Message::text(json)).await.is_err() {
                                consecutive_failures += 1;
                                if consecutive_failures >= MAX_SEND_FAILURES {
                                    tracing::warn!(market_id = %market_id, "WS trade: closing after {MAX_SEND_FAILURES} consecutive send failures");
                                    break;
                                }
                            } else {
                                consecutive_failures = 0;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(market_id = %market_id, lagged = n, "WS client lagged, skipping messages");
                        let _ = send_and_track(
                            &mut ws_tx,
                            Message::text(serde_json::json!({
                                "type": "warning",
                                "message": format!("lagged: skipped {n} messages"),
                            }).to_string()),
                        ).await;
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            // Handle client messages (pong, close).
            msg = ws_rx.next() => {
                match msg {
                    Some(Ok(m)) if m.is_close() => break,
                    Some(Err(_)) | None => break,
                    _ => {} // ignore text/binary from client
                }
            }
            // Graceful shutdown signal.
            _ = shutdown_rx.recv() => {
                let _ = ws_tx.send(Message::close_with(1001u16, "server shutting down")).await;
                break;
            }
        }
    }

    let _ = close_tx.send(());
    ping_task.abort();
    hub.remove_connection(remote_ip);
    tracing::info!(market_id = %market_id, "WS trade feed disconnected");
}

async fn handle_orderbook_ws(ws: WebSocket, market_id: String, remote_ip: IpAddr, hub: Arc<WsHub>) {
    if !hub.add_connection(remote_ip) {
        tracing::warn!("WS connection rejected: max connections reached");
        let (mut tx, _) = ws.split();
        let _ = tx
            .send(Message::close_with(1013u16, "max connections"))
            .await;
        return;
    }

    let mut rx = hub.subscribe_orderbook(&market_id);
    let mut shutdown_rx = hub.subscribe_shutdown();
    let (mut ws_tx, mut ws_rx) = ws.split();

    tracing::info!(market_id = %market_id, "WS orderbook feed connected");

    // Ping task keeps the connection alive (mirrors trade handler).
    let ping_interval = Duration::from_secs(30);
    let (close_tx, mut close_rx) = tokio::sync::oneshot::channel::<()>();
    let ping_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(ping_interval);
        loop {
            tokio::select! {
                _ = interval.tick() => {}
                _ = &mut close_rx => break,
            }
        }
    });

    // Track consecutive send failures for backpressure.
    let mut consecutive_failures: u32 = 0;
    const MAX_SEND_FAILURES: u32 = 5;

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(event) => {
                        if let Ok(json) = serde_json::to_string(&event) {
                            if send_and_track(&mut ws_tx, Message::text(json)).await.is_err() {
                                consecutive_failures += 1;
                                if consecutive_failures >= MAX_SEND_FAILURES {
                                    tracing::warn!(market_id = %market_id, "WS orderbook: closing after {} consecutive send failures", MAX_SEND_FAILURES);
                                    break;
                                }
                            } else {
                                consecutive_failures = 0;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(market_id = %market_id, lagged = n, "WS orderbook client lagged");
                        // Notify the client they missed messages.
                        let _ = send_and_track(
                            &mut ws_tx,
                            Message::text(serde_json::json!({
                                "type": "warning",
                                "message": format!("lagged: skipped {n} messages — next snapshot will be full state"),
                            }).to_string()),
                        ).await;
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            msg = ws_rx.next() => {
                match msg {
                    Some(Ok(m)) if m.is_close() => break,
                    Some(Err(_)) | None => break,
                    _ => {}
                }
            }
            _ = shutdown_rx.recv() => {
                let _ = ws_tx.send(Message::close_with(1001u16, "server shutting down")).await;
                break;
            }
        }
    }

    let _ = close_tx.send(());
    ping_task.abort();
    hub.remove_connection(remote_ip);
    tracing::info!(market_id = %market_id, "WS orderbook feed disconnected");
}

async fn handle_user_ws(ws: WebSocket, user_id: String, remote_ip: IpAddr, hub: Arc<WsHub>) {
    if !hub.add_connection(remote_ip) {
        tracing::warn!("WS connection rejected: max connections reached");
        let (mut tx, _) = ws.split();
        let _ = tx
            .send(Message::close_with(1013u16, "max connections"))
            .await;
        return;
    }

    let mut rx = hub.subscribe_user(&user_id);
    let mut shutdown_rx = hub.subscribe_shutdown();
    let (mut ws_tx, mut ws_rx) = ws.split();

    tracing::info!(user_id = %user_id, "WS user private feed connected");

    let ping_interval = Duration::from_secs(30);
    let (close_tx, mut close_rx) = tokio::sync::oneshot::channel::<()>();
    let ping_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(ping_interval);
        loop {
            tokio::select! {
                _ = interval.tick() => {}
                _ = &mut close_rx => break,
            }
        }
    });

    let mut consecutive_failures: u32 = 0;
    const MAX_SEND_FAILURES: u32 = 5;

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(event) => {
                        if let Ok(json) = serde_json::to_string(&event) {
                            if send_and_track(&mut ws_tx, Message::text(json)).await.is_err() {
                                consecutive_failures += 1;
                                if consecutive_failures >= MAX_SEND_FAILURES {
                                    tracing::warn!(user_id = %user_id, "WS user: closing after {MAX_SEND_FAILURES} consecutive send failures");
                                    break;
                                }
                            } else {
                                consecutive_failures = 0;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(user_id = %user_id, lagged = n, "WS user client lagged");
                        let _ = send_and_track(
                            &mut ws_tx,
                            Message::text(serde_json::json!({
                                "type": "warning",
                                "message": format!("lagged: skipped {n} messages"),
                            }).to_string()),
                        ).await;
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            msg = ws_rx.next() => {
                match msg {
                    Some(Ok(m)) if m.is_close() => break,
                    Some(Err(_)) | None => break,
                    _ => {}
                }
            }
            _ = shutdown_rx.recv() => {
                let _ = ws_tx.send(Message::close_with(1001u16, "server shutting down")).await;
                break;
            }
        }
    }

    let _ = close_tx.send(());
    ping_task.abort();
    hub.remove_connection(remote_ip);
    tracing::info!(user_id = %user_id, "WS user private feed disconnected");
}

async fn handle_ticker_ws(ws: WebSocket, market_id: String, remote_ip: IpAddr, hub: Arc<WsHub>) {
    if !hub.add_connection(remote_ip) {
        let (mut tx, _) = ws.split();
        let _ = tx
            .send(Message::close_with(1013u16, "max connections"))
            .await;
        return;
    }

    let mut rx = hub.subscribe_ticker(&market_id);
    let mut shutdown_rx = hub.subscribe_shutdown();
    let (mut ws_tx, mut ws_rx) = ws.split();

    let mut consecutive_failures: u32 = 0;
    const MAX_SEND_FAILURES: u32 = 5;

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(event) => {
                        if let Ok(json) = serde_json::to_string(&event) {
                            if send_and_track(&mut ws_tx, Message::text(json)).await.is_err() {
                                consecutive_failures += 1;
                                if consecutive_failures >= MAX_SEND_FAILURES { break; }
                            } else {
                                consecutive_failures = 0;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            msg = ws_rx.next() => {
                match msg {
                    Some(Ok(m)) if m.is_close() => break,
                    Some(Err(_)) | None => break,
                    _ => {}
                }
            }
            _ = shutdown_rx.recv() => {
                let _ = ws_tx.send(Message::close_with(1001u16, "server shutting down")).await;
                break;
            }
        }
    }

    hub.remove_connection(remote_ip);
}

async fn handle_liquidation_ws(ws: WebSocket, remote_ip: IpAddr, hub: Arc<WsHub>) {
    if !hub.add_connection(remote_ip) {
        let (mut tx, _) = ws.split();
        let _ = tx
            .send(Message::close_with(1013u16, "max connections"))
            .await;
        return;
    }

    let mut rx = hub.subscribe_liquidations();
    let mut shutdown_rx = hub.subscribe_shutdown();
    let (mut ws_tx, mut ws_rx) = ws.split();

    let mut consecutive_failures: u32 = 0;
    const MAX_SEND_FAILURES: u32 = 5;

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(event) => {
                        if let Ok(json) = serde_json::to_string(&event) {
                            if send_and_track(&mut ws_tx, Message::text(json)).await.is_err() {
                                consecutive_failures += 1;
                                if consecutive_failures >= MAX_SEND_FAILURES { break; }
                            } else {
                                consecutive_failures = 0;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            msg = ws_rx.next() => {
                match msg {
                    Some(Ok(m)) if m.is_close() => break,
                    Some(Err(_)) | None => break,
                    _ => {}
                }
            }
            _ = shutdown_rx.recv() => {
                let _ = ws_tx.send(Message::close_with(1001u16, "server shutting down")).await;
                break;
            }
        }
    }

    hub.remove_connection(remote_ip);
}

async fn handle_mark_price_ws(
    ws: WebSocket,
    market_id: String,
    remote_ip: IpAddr,
    hub: Arc<WsHub>,
) {
    if !hub.add_connection(remote_ip) {
        let (mut tx, _) = ws.split();
        let _ = tx
            .send(Message::close_with(1013u16, "max connections"))
            .await;
        return;
    }

    let mut rx = hub.subscribe_mark_price(&market_id);
    let mut shutdown_rx = hub.subscribe_shutdown();
    let (mut ws_tx, mut ws_rx) = ws.split();
    let mut consecutive_failures: u32 = 0;
    const MAX_SEND_FAILURES: u32 = 5;

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(event) => {
                        if let Ok(json) = serde_json::to_string(&event) {
                            if send_and_track(&mut ws_tx, Message::text(json)).await.is_err() {
                                consecutive_failures += 1;
                                if consecutive_failures >= MAX_SEND_FAILURES { break; }
                            } else {
                                consecutive_failures = 0;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            msg = ws_rx.next() => {
                match msg {
                    Some(Ok(m)) if m.is_close() => break,
                    Some(Err(_)) | None => break,
                    _ => {}
                }
            }
            _ = shutdown_rx.recv() => {
                let _ = ws_tx.send(Message::close_with(1001u16, "server shutting down")).await;
                break;
            }
        }
    }

    hub.remove_connection(remote_ip);
}

fn publish_event_to_ws_hub(hub: &WsHub, event: types::Event) {
    match event {
        types::Event::FillCreated(fill) => {
            let data = serde_json::json!({
                "trade_id": fill.id,
                "price": fill.price,
                "amount": fill.amount,
                "side": format!("{:?}", fill.side),
                "timestamp": fill.timestamp,
            });
            hub.publish_trade(&fill.market_id, data);
            hub.publish_ticker(
                &fill.market_id,
                serde_json::json!({
                    "last_price": fill.price,
                    "last_amount": fill.amount,
                    "side": format!("{:?}", fill.side),
                    "timestamp": fill.timestamp,
                }),
            );
            hub.publish_user_event(
                &fill.user_id,
                WsFeedEvent {
                    event_type: "fill".into(),
                    market_id: fill.market_id.clone(),
                    data: serde_json::json!({
                        "trade_id": fill.id,
                        "side": format!("{:?}", fill.side),
                        "price": fill.price,
                        "amount": fill.amount,
                        "fee": fill.fee,
                        "is_maker": fill.is_maker,
                        "timestamp": fill.timestamp,
                    }),
                },
            );
        }
        types::Event::LedgerCommitted(delta) => {
            let mut notified = std::collections::HashSet::new();
            for entry in &delta.entries {
                for acct in [&entry.debit_account, &entry.credit_account] {
                    if let Some(user_id) = acct.strip_prefix("U:") {
                        if let Some(uid) = user_id.split(':').next() {
                            if notified.insert(uid.to_string()) {
                                hub.publish_user_event(
                                    uid,
                                    WsFeedEvent {
                                        event_type: "balance_update".into(),
                                        market_id: String::new(),
                                        data: serde_json::json!({
                                            "op_id": delta.op_id,
                                            "timestamp": delta.timestamp,
                                        }),
                                    },
                                );
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

/// Bridge: subscribes to eventbus fill and ledger events and publishes them to the WsHub.
pub async fn bridge_eventbus_to_ws(eventbus: eventbus::EventBus, hub: Arc<WsHub>) {
    let mut fill_rx = eventbus.subscribe("fill.created");
    let mut ledger_rx = eventbus.subscribe("ledger.committed");
    let mut shutdown_rx = hub.subscribe_shutdown();
    observability::METRICS
        .bridge_alive
        .store(true, std::sync::atomic::Ordering::Relaxed);
    loop {
        tokio::select! {
            result = fill_rx.recv() => {
                match result {
                    Ok(types::Event::FillCreated(fill)) => {
                        let data = serde_json::json!({
                            "trade_id": fill.id,
                            "price": fill.price,
                            "amount": fill.amount,
                            "side": format!("{:?}", fill.side),
                            "timestamp": fill.timestamp,
                        });
                        hub.publish_trade(&fill.market_id, data);
                        // Push ticker update on each trade
                        hub.publish_ticker(&fill.market_id, serde_json::json!({
                            "last_price": fill.price,
                            "last_amount": fill.amount,
                            "side": format!("{:?}", fill.side),
                            "timestamp": fill.timestamp,
                        }));
                        // Push to user's private stream
                        hub.publish_user_event(&fill.user_id, WsFeedEvent {
                            event_type: "fill".into(),
                            market_id: fill.market_id.clone(),
                            data: serde_json::json!({
                                "trade_id": fill.id,
                                "side": format!("{:?}", fill.side),
                                "price": fill.price,
                                "amount": fill.amount,
                                "fee": fill.fee,
                                "is_maker": fill.is_maker,
                                "timestamp": fill.timestamp,
                            }),
                        });
                    }
                    Ok(types::Event::LedgerCommitted(delta)) => {
                        // Extract affected user IDs from ledger entries and
                        // push a balance_update event to each user's private
                        // WS stream.
                        let mut notified = std::collections::HashSet::new();
                        for entry in &delta.entries {
                            for acct in [&entry.debit_account, &entry.credit_account] {
                                if let Some(user_id) = acct.strip_prefix("U:") {
                                    if let Some(uid) = user_id.split(':').next() {
                                        if notified.insert(uid.to_string()) {
                                            hub.publish_user_event(uid, WsFeedEvent {
                                                event_type: "balance_update".into(),
                                                market_id: String::new(),
                                                data: serde_json::json!({
                                                    "op_id": delta.op_id,
                                                    "timestamp": delta.timestamp,
                                                }),
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Ok(_) => {} // other event types — ignore
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(lagged = n, "eventbus→ws bridge lagged");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            result = ledger_rx.recv() => {
                match result {
                    Ok(event) => publish_event_to_ws_hub(hub.as_ref(), event),
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(lagged = n, stream = "ledger.committed", "eventbus->ws bridge lagged");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            _ = shutdown_rx.recv() => break,
        }
    }
    observability::METRICS
        .bridge_alive
        .store(false, std::sync::atomic::Ordering::Relaxed);
}

/// Periodically capture orderbook snapshots from the matching engine and push
/// them to all `/ws/orderbook/:market_id` subscribers.
pub async fn run_orderbook_snapshot_scheduler(
    engine: Arc<matching::PartitionedMatchingEngine>,
    hub: Arc<WsHub>,
    interval_ms: u64,
) {
    if interval_ms == 0 {
        tracing::info!("orderbook snapshot scheduler disabled (interval_ms=0)");
        return;
    }

    let mut interval = tokio::time::interval(Duration::from_millis(interval_ms));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut shutdown_rx = hub.subscribe_shutdown();
    let depth: usize = 20;

    tracing::info!(interval_ms, "orderbook snapshot scheduler started");

    loop {
        tokio::select! {
            _ = interval.tick() => {
                // Only publish if someone is actually listening.
                if hub.orderbook_channels.read().is_empty() {
                    continue;
                }
                let records = match engine.export_snapshots().await {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::warn!(error = %e, "orderbook snapshot export failed");
                        continue;
                    }
                };
                for record in &records {
                    for market in &record.snapshot.markets {
                        let book = build_orderbook_snapshot(market, depth);
                        hub.publish_orderbook(&market.market_id, book);
                    }
                }
            }
            _ = shutdown_rx.recv() => break,
        }
    }
}

fn build_orderbook_snapshot(
    market: &matching::MarketRuntimeSnapshot,
    depth: usize,
) -> serde_json::Value {
    let mut bids: std::collections::BTreeMap<i64, i64> = std::collections::BTreeMap::new();
    let mut asks: std::collections::BTreeMap<i64, i64> = std::collections::BTreeMap::new();
    for order in &market.orders {
        match order.side {
            types::Side::Buy => *bids.entry(order.price).or_default() += order.remaining_amount,
            types::Side::Sell => *asks.entry(order.price).or_default() += order.remaining_amount,
        }
    }
    // Bids: highest first, asks: lowest first.
    let bid_levels: Vec<[i64; 2]> = bids
        .into_iter()
        .rev()
        .take(depth)
        .map(|(p, q)| [p, q])
        .collect();
    let ask_levels: Vec<[i64; 2]> = asks.into_iter().take(depth).map(|(p, q)| [p, q]).collect();
    serde_json::json!({
        "market_id": market.market_id,
        "outcome": market.outcome,
        "bids": bid_levels,
        "asks": ask_levels,
        "timestamp": chrono::Utc::now(),
    })
}

/// Periodically compute and broadcast mark prices for all active markets.
pub async fn run_mark_price_scheduler(
    engine: Arc<matching::PartitionedMatchingEngine>,
    index_prices: Arc<super::pricing::PersistentIndexPriceStore>,
    hub: Arc<WsHub>,
    interval_ms: u64,
) {
    if interval_ms == 0 {
        tracing::info!("mark price scheduler disabled (interval_ms=0)");
        return;
    }

    let mut interval = tokio::time::interval(Duration::from_millis(interval_ms));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut shutdown_rx = hub.subscribe_shutdown();

    tracing::info!(interval_ms, "mark price scheduler started");

    loop {
        tokio::select! {
            _ = interval.tick() => {
                if hub.mark_price_channels.read().is_empty() {
                    continue;
                }
                let records = match engine.export_snapshots().await {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::warn!(error = %e, "mark price snapshot export failed");
                        continue;
                    }
                };
                let snapshots = super::flatten_market_snapshots(&records);
                for snapshot in &snapshots {
                    if let Some(quote) = super::pricing::fair_price_quote_for_snapshot(snapshot, &index_prices) {
                        hub.publish_mark_price(
                            &snapshot.market_id,
                            serde_json::json!({
                                "market_id": snapshot.market_id,
                                "outcome": snapshot.outcome,
                                "mark_price": quote.fair_price,
                                "timestamp": chrono::Utc::now(),
                            }),
                        );
                    }
                }
            }
            _ = shutdown_rx.recv() => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use tokio::time::{timeout, Duration};
    use types::{Event, Fill, LedgerDelta, LedgerEntry, SettlementStatus, Side};

    #[tokio::test]
    async fn bridge_publishes_balance_updates_for_ledger_commits() {
        let bus = eventbus::EventBus::new();
        let hub = Arc::new(WsHub::new());
        let mut user_rx = hub.subscribe_user("alice");
        let bridge = tokio::spawn(bridge_eventbus_to_ws(bus.clone(), hub.clone()));

        tokio::task::yield_now().await;

        bus.publish(Event::LedgerCommitted(LedgerDelta {
            op_id: "dep-alice-1".into(),
            entries: vec![LedgerEntry {
                debit_account: "SYS:VAULT:USDC".into(),
                credit_account: "U:alice:USDC".into(),
                amount: 1_000,
                op_id: "dep-alice-1".into(),
                timestamp: Utc::now(),
            }],
            timestamp: Utc::now(),
        }));

        let event = timeout(Duration::from_secs(1), user_rx.recv())
            .await
            .expect("balance update arrives")
            .expect("bridge event");

        assert_eq!(event.event_type, "balance_update");
        assert_eq!(event.data["op_id"], "dep-alice-1");

        hub.shutdown();
        let _ = bridge.await;
    }

    #[tokio::test]
    async fn bridge_still_publishes_fill_events_to_user_stream() {
        let bus = eventbus::EventBus::new();
        let hub = Arc::new(WsHub::new());
        let mut user_rx = hub.subscribe_user("maker-1");
        let bridge = tokio::spawn(bridge_eventbus_to_ws(bus.clone(), hub.clone()));

        tokio::task::yield_now().await;

        bus.publish(Event::FillCreated(Fill {
            id: "fill-1".into(),
            intent_id: "intent-1".into(),
            user_id: "maker-1".into(),
            market_id: "btc-usdt".into(),
            side: Side::Sell,
            price: 50_000,
            amount: 2,
            outcome: 0,
            timestamp: Utc::now(),
            op_id: "fill-op-1".into(),
            fee: 1,
            fee_bps: 5,
            is_maker: true,
            aggressor_side: Some(Side::Buy),
            fill_index: 0,
            settlement_status: SettlementStatus::Settled,
        }));

        let event = timeout(Duration::from_secs(1), user_rx.recv())
            .await
            .expect("fill event arrives")
            .expect("bridge event");

        assert_eq!(event.event_type, "fill");
        assert_eq!(event.market_id, "btc-usdt");
        assert_eq!(event.data["trade_id"], "fill-1");

        hub.shutdown();
        let _ = bridge.await;
    }
}
