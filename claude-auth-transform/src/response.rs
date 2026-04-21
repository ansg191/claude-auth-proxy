use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use bytes::{Bytes, BytesMut};
use http::Response;
use http_body::{Body, Frame, SizeHint};
use regex::Regex;
use tracing::{debug, trace};

use crate::tool_names::ToolNameMapper;

static TOOL_PREFIX_RE: std::sync::LazyLock<Regex> =
    std::sync::LazyLock::new(|| Regex::new(r#""name"\s*:\s*"(t_[0-9a-f]+)""#).unwrap());

pub fn transform_response<B>(
    mut response: Response<B>,
    tool_name_mapper: Arc<ToolNameMapper>,
) -> Response<ClaudeBody<B>>
where
    B: Body,
{
    // Remove stale headers
    let headers = response.headers_mut();
    headers.remove("Content-Length");
    headers.remove("Transfer-Encoding");

    response.map(|body| ClaudeBody {
        inner: body,
        buffer: BytesMut::new(),
        tool_name_mapper,
    })
}

/// Strips `"name": "t_<hash>"` -> `"name": "<tool>"` from a byte slice.
fn strip_tool_prefix(input: &[u8], tool_name_mapper: &ToolNameMapper) -> Bytes {
    let text = String::from_utf8_lossy(input);
    let result = TOOL_PREFIX_RE.replace_all(&text, |caps: &regex::Captures<'_>| {
        format!(r#""name": "{}""#, tool_name_mapper.deobfuscate(&caps[1]))
    });
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
    tool_name_mapper: Arc<ToolNameMapper>,
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
                let stripped = strip_tool_prefix(&event_bytes, this.tool_name_mapper.as_ref());
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
                        let stripped =
                            strip_tool_prefix(&remaining, this.tool_name_mapper.as_ref());
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
        let tool_name_mapper = ToolNameMapper::new(8, 16);
        let obfuscated = tool_name_mapper.obfuscate("search");
        let input = format!(r#"{{"name": "{obfuscated}", "id": "123"}}"#);
        let result =
            String::from_utf8(strip_tool_prefix(input.as_bytes(), &tool_name_mapper).to_vec())
                .unwrap();
        assert_eq!(result, r#"{"name": "search", "id": "123"}"#);
    }

    #[test]
    fn strip_prefix_multiple_occurrences() {
        let tool_name_mapper = ToolNameMapper::new(8, 16);
        let foo = tool_name_mapper.obfuscate("foo");
        let bar = tool_name_mapper.obfuscate("bar");
        let input = format!(r#"{{"name": "{foo}"}} {{"name": "{bar}"}}"#);
        let result =
            String::from_utf8(strip_tool_prefix(input.as_bytes(), &tool_name_mapper).to_vec())
                .unwrap();
        assert_eq!(result, r#"{"name": "foo"} {"name": "bar"}"#);
    }

    #[test]
    fn no_prefix_unchanged() {
        let input = r#"{"name": "search", "id": "123"}"#;
        let result = String::from_utf8(
            strip_tool_prefix(input.as_bytes(), &ToolNameMapper::new(8, 16)).to_vec(),
        )
        .unwrap();
        assert_eq!(result, input);
    }

    #[test]
    fn strip_prefix_with_whitespace_around_colon() {
        let tool_name_mapper = ToolNameMapper::new(8, 16);
        let obfuscated = tool_name_mapper.obfuscate("tool");
        let input = format!(r#"{{"name" : "{obfuscated}"}}"#);
        let result =
            String::from_utf8(strip_tool_prefix(input.as_bytes(), &tool_name_mapper).to_vec())
                .unwrap();
        assert_eq!(result, r#"{"name": "tool"}"#);
    }

    #[test]
    fn does_not_strip_non_name_fields() {
        let tool_name_mapper = ToolNameMapper::new(8, 16);
        let obfuscated = tool_name_mapper.obfuscate("tool");
        let input = format!(r#"{{"id": "mcp_123", "name": "{obfuscated}"}}"#);
        let result =
            String::from_utf8(strip_tool_prefix(input.as_bytes(), &tool_name_mapper).to_vec())
                .unwrap();
        assert_eq!(result, r#"{"id": "mcp_123", "name": "tool"}"#);
    }

    #[test]
    fn strip_prefix_restores_snake_case_after_obfuscation() {
        let tool_name_mapper = ToolNameMapper::new(8, 16);
        let obfuscated = tool_name_mapper.obfuscate("background_output");
        let input = format!(r#"{{"name": "{obfuscated}"}}"#);
        let result =
            String::from_utf8(strip_tool_prefix(input.as_bytes(), &tool_name_mapper).to_vec())
                .unwrap();
        assert_eq!(result, r#"{"name": "background_output"}"#);
    }

    #[test]
    fn find_double_newline_finds_boundary() {
        assert_eq!(find_double_newline(b"data: hello\n\ndata: world"), Some(11));
    }

    #[test]
    fn find_double_newline_none_when_absent() {
        assert_eq!(find_double_newline(b"data: hello\ndata: world"), None);
    }

    /// Regression test: upstream responses carry a `Content-Length` (and
    /// sometimes `Transfer-Encoding`) matching the pre-transform body. Because
    /// `strip_tool_prefix` rewrites `"name":"t_<hash>"` to `"name": "<real>"`,
    /// the transformed body length changes. If we forwarded the stale header,
    /// hyper would truncate the body on the wire and clients would see an
    /// unterminated JSON string. `transform_response` must strip both headers
    /// so hyper frames the transformed body with chunked transfer encoding.
    #[tokio::test]
    async fn transform_response_strips_stale_length_headers_and_deobfuscates_body() {
        use http::Response;
        use http_body_util::{BodyExt, Full};

        let mapper = Arc::new(ToolNameMapper::new(8, 16));
        let obfuscated = mapper.obfuscate("kubernetes_tabular_query");
        let upstream_body = format!(r#"{{"name":"{obfuscated}"}}"#);
        let upstream_len = upstream_body.len();

        let response = Response::builder()
            .header("content-length", upstream_len.to_string())
            .header("transfer-encoding", "chunked")
            .header("content-type", "application/json")
            .body(Full::new(Bytes::from(upstream_body)))
            .unwrap();

        let out = transform_response(response, mapper);

        assert!(
            out.headers().get("content-length").is_none(),
            "content-length must be stripped; otherwise hyper truncates the body"
        );
        assert!(
            out.headers().get("transfer-encoding").is_none(),
            "transfer-encoding must be stripped; it's hop-by-hop per RFC 7230"
        );
        assert_eq!(
            out.headers().get("content-type").unwrap(),
            "application/json"
        );

        let collected = out.into_body().collect().await.unwrap().to_bytes();
        let body_str = std::str::from_utf8(&collected).unwrap();
        assert_eq!(body_str, r#"{"name": "kubernetes_tabular_query"}"#);

        assert!(
            collected.len() > upstream_len,
            "transformed body ({} bytes) must exceed stale content-length ({} bytes) \
             for this regression to be meaningful",
            collected.len(),
            upstream_len,
        );
    }
}
