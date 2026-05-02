//! Custom SSE (Server-Sent Events) parser.
//!
//! Replaces `eventsource-stream` with a lightweight, controllable
//! implementation that parses SSE events from a byte stream.

use bytes::Bytes;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio_stream::Stream;

/// A parsed SSE event.
#[derive(Debug, Clone, Default)]
pub struct SseEvent {
    pub event: String,
    pub data: String,
}

const MAX_BUFFER_SIZE: usize = 1024 * 1024; // 1MB limit

pin_project_lite::pin_project! {
    /// A stream that parses SSE events from a byte stream.
    pub struct SseStream<S> {
        #[pin]
        inner: S,
        buffer: String,
    }
}

impl<S> SseStream<S> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            buffer: String::new(),
        }
    }
}

impl<S, E> Stream for SseStream<S>
where
    S: Stream<Item = Result<Bytes, E>>,
    E: std::fmt::Display,
{
    type Item = Result<SseEvent, E>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        loop {
            if let Some(pos) = this.buffer.find("\n\n") {
                let event_text = this.buffer[..pos].to_string();
                this.buffer.drain(..pos + 2);
                let event = parse_sse_event(&event_text);
                if !event.data.is_empty() {
                    return Poll::Ready(Some(Ok(event)));
                }
                continue;
            }

            match this.inner.as_mut().poll_next(cx) {
                Poll::Ready(Some(Ok(bytes))) => {
                    if this.buffer.len() + bytes.len() > MAX_BUFFER_SIZE {
                        tracing::error!(
                            msg = "SSE buffer overflow, clearing buffer",
                            buffer_size = this.buffer.len(),
                            incoming_bytes = bytes.len(),
                            max_size = MAX_BUFFER_SIZE,
                        );
                        this.buffer.clear();
                    }
                    this.buffer.push_str(&String::from_utf8_lossy(&bytes));
                }
                Poll::Ready(Some(Err(e))) => {
                    return Poll::Ready(Some(Err(e)));
                }
                Poll::Ready(None) => {
                    if !this.buffer.is_empty() {
                        let event = parse_sse_event(this.buffer);
                        this.buffer.clear();
                        if !event.data.is_empty() {
                            return Poll::Ready(Some(Ok(event)));
                        }
                    }
                    return Poll::Ready(None);
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

fn parse_sse_event(text: &str) -> SseEvent {
    let mut event = SseEvent::default();
    for line in text.lines() {
        if let Some(data_value) = line.strip_prefix("data:") {
            let data_value = data_value.strip_prefix(' ').unwrap_or(data_value);
            if !event.data.is_empty() {
                event.data.push('\n');
            }
            event.data.push_str(data_value);
        } else if let Some(event_value) = line.strip_prefix("event:") {
            event.event = event_value
                .strip_prefix(' ')
                .unwrap_or(event_value)
                .to_string();
        }
    }
    event
}

/// Extension trait to convert a byte stream into an SSE stream.
pub trait IntoSseStream {
    fn into_sse_stream(self) -> SseStream<Self>
    where
        Self: Sized;
}

impl<S> IntoSseStream for S {
    fn into_sse_stream(self) -> SseStream<Self>
    where
        Self: Sized,
    {
        SseStream::new(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_stream::StreamExt;

    fn bytes_stream_from(
        data: Vec<&'static str>,
    ) -> impl Stream<Item = Result<Bytes, std::convert::Infallible>> {
        let items: Vec<Result<Bytes, std::convert::Infallible>> =
            data.into_iter().map(|s| Ok(Bytes::from(s))).collect();
        tokio_stream::iter(items)
    }

    #[tokio::test]
    async fn test_basic_event_parsing() {
        let stream = bytes_stream_from(vec!["data: hello\n\n"]);
        let mut sse = stream.into_sse_stream();
        let event = sse.next().await.unwrap().unwrap();
        assert_eq!(event.data, "hello");
        assert_eq!(event.event, "");
    }

    #[tokio::test]
    async fn test_event_with_type() {
        let stream = bytes_stream_from(vec!["event: message_start\ndata: {\"id\":\"1\"}\n\n"]);
        let mut sse = stream.into_sse_stream();
        let event = sse.next().await.unwrap().unwrap();
        assert_eq!(event.event, "message_start");
        assert_eq!(event.data, "{\"id\":\"1\"}");
    }

    #[tokio::test]
    async fn test_multiple_events_in_one_chunk() {
        let stream = bytes_stream_from(vec!["data: first\n\ndata: second\n\n"]);
        let mut sse = stream.into_sse_stream();
        let e1 = sse.next().await.unwrap().unwrap();
        assert_eq!(e1.data, "first");
        let e2 = sse.next().await.unwrap().unwrap();
        assert_eq!(e2.data, "second");
    }

    #[tokio::test]
    async fn test_event_split_across_chunks() {
        let stream = bytes_stream_from(vec!["data: hel", "lo\n\n"]);
        let mut sse = stream.into_sse_stream();
        let event = sse.next().await.unwrap().unwrap();
        assert_eq!(event.data, "hello");
    }

    #[tokio::test]
    async fn test_multiline_data() {
        let stream = bytes_stream_from(vec!["data: line1\ndata: line2\n\n"]);
        let mut sse = stream.into_sse_stream();
        let event = sse.next().await.unwrap().unwrap();
        assert_eq!(event.data, "line1\nline2");
    }

    #[tokio::test]
    async fn test_done_sentinel() {
        let stream = bytes_stream_from(vec!["data: [DONE]\n\n"]);
        let mut sse = stream.into_sse_stream();
        let event = sse.next().await.unwrap().unwrap();
        assert_eq!(event.data, "[DONE]");
    }

    #[tokio::test]
    async fn test_empty_data_skipped() {
        let stream = bytes_stream_from(vec!["event: ping\n\ndata: real\n\n"]);
        let mut sse = stream.into_sse_stream();
        let event = sse.next().await.unwrap().unwrap();
        assert_eq!(event.data, "real");
        assert_eq!(event.event, "");
    }

    #[tokio::test]
    async fn test_trailing_data_without_final_newlines() {
        let stream = bytes_stream_from(vec!["data: trailing"]);
        let mut sse = stream.into_sse_stream();
        let event = sse.next().await.unwrap().unwrap();
        assert_eq!(event.data, "trailing");
    }

    #[tokio::test]
    async fn test_buffer_overflow_protection() {
        // Create a large chunk that exceeds MAX_BUFFER_SIZE without \n\n delimiter
        // Use a static string to avoid lifetime issues
        let large_data: &'static str =
            Box::leak("x".repeat(MAX_BUFFER_SIZE + 100).into_boxed_str());
        let stream = bytes_stream_from(vec![large_data, "data: valid\n\n"]);
        let mut sse = stream.into_sse_stream();

        // Should still parse valid events after buffer clear
        let event = sse.next().await.unwrap().unwrap();
        assert_eq!(event.data, "valid");
    }
}
