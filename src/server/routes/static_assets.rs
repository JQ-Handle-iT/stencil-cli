use axum::{
    extract::{
        ws::{Message, WebSocket},
        State, WebSocketUpgrade,
    },
    response::Response,
};
use futures::StreamExt;

use crate::server::state::{AppState, LiveReloadMessage};

/// WebSocket handler for live reload
pub async fn live_reload_ws(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> Response {
    let tx = state.live_reload_tx.clone();
    ws.on_upgrade(move |socket| handle_ws(socket, tx.subscribe()))
}

async fn handle_ws(
    mut socket: WebSocket,
    mut rx: tokio::sync::broadcast::Receiver<LiveReloadMessage>,
) {
    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Ok(LiveReloadMessage::FullReload) => {
                        if socket.send(Message::Text(r#"{"type":"full"}"#.into())).await.is_err() {
                            break;
                        }
                    }
                    Ok(LiveReloadMessage::CssReload) => {
                        if socket.send(Message::Text(r#"{"type":"css"}"#.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            msg = socket.next() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }
}
