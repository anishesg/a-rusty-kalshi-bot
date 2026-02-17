use crate::state::{AppState, WsMessage};
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;

/// WebSocket upgrade handler
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.ws_tx.subscribe();

    // Send initial snapshot
    {
        let snapshot = state.snapshot_rx.borrow().clone();
        if let Ok(json) = serde_json::to_string(&snapshot) {
            let msg = Message::Text(json.into());
            if sender.send(msg).await.is_err() {
                return;
            }
        }
    }

    // Forward broadcast messages to this client
    let send_task = tokio::spawn(async move {
        while let Ok(ws_msg) = rx.recv().await {
            match serde_json::to_string(&ws_msg) {
                Ok(json) => {
                    if sender.send(Message::Text(json.into())).await.is_err() {
                        break;
                    }
                }
                Err(_) => continue,
            }
        }
    });

    // Read (and discard) incoming messages; detect disconnect
    let recv_task = tokio::spawn(async move {
        while let Some(msg) = receiver.next().await {
            match msg {
                Ok(Message::Close(_)) | Err(_) => break,
                _ => {} // Ignore client messages
            }
        }
    });

    // Wait for either task to finish (client disconnected)
    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }
}
