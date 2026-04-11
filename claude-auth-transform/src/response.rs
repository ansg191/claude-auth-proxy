use std::{
    pin::Pin,
    task::{Context, Poll},
};

use bytes::{Bytes, BytesMut};
use http::Response;
use http_body::{Body, Frame, SizeHint};
use regex::Regex;
use tracing::{debug, trace};

static TOOL_PREFIX_RE: std::sync::LazyLock<Regex> =
    std::sync::LazyLock::new(|| Regex::new(r#""name"\s*:\s*"mcp_([^"]+)""#).unwrap());

pub fn transform_response<B>(response: Response<B>) -> Response<ClaudeBody<B>>
where
    B: Body,
{
    response.map(|body| ClaudeBody {
        inner: body,
        buffer: BytesMut::new(),
    })
}

/// Strips `"name": "mcp_<tool>"` -> `"name": "<tool>"` from a byte slice.
fn strip_tool_prefix(input: &[u8]) -> Bytes {
    let text = String::from_utf8_lossy(input);
    let result = TOOL_PREFIX_RE.replace_all(&text, r#""name": "$1""#);
    Bytes::from(result.into_owned())
}

/// Claude API Response Body
///
/// This is an `http_body::Body` that handles both simple responses and SSE streaming.
///
/// For SSE streams it buffers until a complete event boundary (`\n\n`) is found,
/// strips the `mcp_` tool prefix from each event, then forwards the frame.
///
/// For non-SSE / error responses, each data frame is passed through with only
/// tool-prefix stripping applied.
#[pin_project::pin_project]
pub struct ClaudeBody<B> {
    /// The underlying [`http_body::Body`]
    #[pin]
    inner: B,
    /// SSE line buffer for reassembling partial events
    buffer: BytesMut,
}

impl<B> Body for ClaudeBody<B>
where
    B: Body,
    B::Data: AsRef<[u8]>,
    B::Error: std::fmt::Debug,
{
    type Data = Bytes;
    type Error = B::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let mut this = self.project();

        loop {
            // Check if the buffer already contains a complete SSE event
            if let Some(boundary) = find_double_newline(this.buffer) {
                let event_bytes = this.buffer.split_to(boundary + 2);
                trace!(raw = %String::from_utf8_lossy(&event_bytes), "Raw SSE event");
                let stripped = strip_tool_prefix(&event_bytes);
                return Poll::Ready(Some(Ok(Frame::data(stripped))));
            }

            // Poll the inner body for more data
            match this.inner.as_mut().poll_frame(cx) {
                Poll::Ready(Some(Ok(frame))) => {
                    if let Some(data) = frame.data_ref() {
                        let chunk = data.as_ref();
                        trace!(len = chunk.len(), raw = %String::from_utf8_lossy(chunk), "Received chunk");
                        this.buffer.extend_from_slice(chunk);
                        // Loop back to check for a complete event
                    } else {
                        // Trailers frame, forward as-is
                        if let Ok(trailers) = frame.into_trailers() {
                            debug!("Forwarding trailers");
                            return Poll::Ready(Some(Ok(Frame::trailers(trailers))));
                        }
                    }
                }
                Poll::Ready(Some(Err(e))) => {
                    debug!(?e, "Inner body error");
                    return Poll::Ready(Some(Err(e)));
                }
                Poll::Ready(None) => {
                    // Inner body is done, flush any remaining buffered data
                    if !this.buffer.is_empty() {
                        let remaining = this.buffer.split();
                        trace!(raw = %String::from_utf8_lossy(&remaining), "Flushing remaining buffer");
                        let stripped = strip_tool_prefix(&remaining);
                        return Poll::Ready(Some(Ok(Frame::data(stripped))));
                    }
                    debug!("Response body complete");
                    return Poll::Ready(None);
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }

    fn size_hint(&self) -> SizeHint {
        SizeHint::default()
    }
}

/// Find the position of the first `\n\n` boundary in the buffer.
fn find_double_newline(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|w| w == b"\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_prefix_from_name_field() {
        let input = r#"{"name": "mcp_search", "id": "123"}"#;
        let result = String::from_utf8(strip_tool_prefix(input.as_bytes()).to_vec()).unwrap();
        assert_eq!(result, r#"{"name": "search", "id": "123"}"#);
    }

    #[test]
    fn strip_prefix_multiple_occurrences() {
        let input = r#"{"name": "mcp_foo"} {"name": "mcp_bar"}"#;
        let result = String::from_utf8(strip_tool_prefix(input.as_bytes()).to_vec()).unwrap();
        assert_eq!(result, r#"{"name": "foo"} {"name": "bar"}"#);
    }

    #[test]
    fn no_prefix_unchanged() {
        let input = r#"{"name": "search", "id": "123"}"#;
        let result = String::from_utf8(strip_tool_prefix(input.as_bytes()).to_vec()).unwrap();
        assert_eq!(result, input);
    }

    #[test]
    fn strip_prefix_with_whitespace_around_colon() {
        let input = r#"{"name" : "mcp_tool"}"#;
        let result = String::from_utf8(strip_tool_prefix(input.as_bytes()).to_vec()).unwrap();
        assert_eq!(result, r#"{"name": "tool"}"#);
    }

    #[test]
    fn does_not_strip_non_name_fields() {
        let input = r#"{"id": "mcp_123", "name": "mcp_tool"}"#;
        let result = String::from_utf8(strip_tool_prefix(input.as_bytes()).to_vec()).unwrap();
        assert_eq!(result, r#"{"id": "mcp_123", "name": "tool"}"#);
    }

    #[test]
    fn find_double_newline_finds_boundary() {
        assert_eq!(find_double_newline(b"data: hello\n\ndata: world"), Some(11));
    }

    #[test]
    fn find_double_newline_none_when_absent() {
        assert_eq!(find_double_newline(b"data: hello\ndata: world"), None);
    }
}
