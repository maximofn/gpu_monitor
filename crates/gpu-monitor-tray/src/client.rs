use std::time::Duration;

use anyhow::Result;
use futures_util::StreamExt;
use gpu_monitor_core::Snapshot;
use reqwest_eventsource::{Event, EventSource};
use tokio::sync::mpsc;

const INITIAL_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub enum Update {
    Connected(Snapshot),
    Disconnected(String),
}

pub fn spawn(backend_url: String, tx: mpsc::Sender<Update>) {
    tokio::spawn(async move {
        let stream_url = format!("{}/v1/stream", backend_url.trim_end_matches('/'));
        let mut backoff = INITIAL_BACKOFF;
        loop {
            let outcome = run_once(&stream_url, &tx, &mut backoff).await;
            match outcome {
                Ok(()) => break,
                Err(err) => {
                    tracing::warn!(error = %err, "SSE session ended; reconnecting");
                    if tx.send(Update::Disconnected(err.to_string())).await.is_err() {
                        break;
                    }
                }
            }
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(MAX_BACKOFF);
        }
    });
}

async fn run_once(
    stream_url: &str,
    tx: &mpsc::Sender<Update>,
    backoff: &mut Duration,
) -> Result<()> {
    tracing::info!(url = %stream_url, "connecting to backend");
    let mut es = EventSource::get(stream_url);
    while let Some(event) = es.next().await {
        match event {
            Ok(Event::Open) => {
                tracing::info!("SSE stream open");
                *backoff = INITIAL_BACKOFF;
            }
            Ok(Event::Message(msg)) => match serde_json::from_str::<Snapshot>(&msg.data) {
                Ok(snapshot) => {
                    *backoff = INITIAL_BACKOFF;
                    if tx.send(Update::Connected(snapshot)).await.is_err() {
                        return Ok(());
                    }
                }
                Err(err) => {
                    tracing::warn!(error = %err, "could not decode snapshot event");
                }
            },
            Err(err) => {
                es.close();
                return Err(err.into());
            }
        }
    }
    Ok(())
}
