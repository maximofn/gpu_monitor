use std::convert::Infallible;
use std::time::Duration;

use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::stream::Stream;
use futures_util::StreamExt;
use tokio_stream::wrappers::WatchStream;

use super::AppState;

pub async fn stream(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let watch = WatchStream::new(state.snapshot_rx.clone());
    let stream = watch.map(|snapshot| {
        let event = match Event::default().json_data(&snapshot) {
            Ok(ev) => ev,
            Err(err) => {
                tracing::warn!(error = %err, "failed to encode SSE event");
                Event::default().comment("encode error")
            }
        };
        Ok::<_, Infallible>(event)
    });
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}
