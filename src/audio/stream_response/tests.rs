use super::super::error::{classify_anyhow, classify_io};
use super::*;
use crate::audio::http_stream::tests::serve;
use crate::audio::preload::tests::client;

fn metadata(range: &str, payload_len: Option<u64>, position: u64) -> Result<ResponseMetadata> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_RANGE, range.parse().unwrap());
    ResponseMetadata::inspect(StatusCode::PARTIAL_CONTENT, &headers, payload_len, position)
}

#[test]
fn parses_inclusive_ranges_known_unknown_and_large_totals() {
    for (value, start, total, count) in [
        ("bytes 0-0/1", 0, Some(1), 1),
        ("bytes 5-9/10", 5, Some(10), 5),
        ("bytes 5-9/*", 5, None, 5),
        (" BYTES\t0-4/5 ", 0, Some(5), 5),
        (
            "bytes 18446744073709551614-18446744073709551614/18446744073709551615",
            u64::MAX - 1,
            Some(u64::MAX),
            1,
        ),
        (
            "bytes 18446744073709551615-18446744073709551615/*",
            u64::MAX,
            None,
            1,
        ),
    ] {
        assert_eq!(
            ByteRange::parse(value),
            Some(ByteRange {
                start,
                total,
                count
            }),
            "{value}"
        );
    }
}

#[test]
fn malformed_ranges_cannot_supply_a_resource_length_or_read_limit() {
    for value in [
        "",
        "bytes",
        "bytes */5",
        "items 0-4/5",
        "bytes 0-4",
        "bytes 0/5",
        "bytes 4-0/5",
        "bytes 0-5/5",
        "bytes 0-0/0",
        "bytes 0-4/-5",
        "bytes +0-4/5",
        "bytes 0-+4/5",
        "bytes 0-4/+5",
        "bytes 0-4/5 extra",
        "bytes 0-1/5, 3-4/5",
        "bytes 0-18446744073709551615/*",
        "bytes 18446744073709551616-18446744073709551616/*",
    ] {
        assert!(ByteRange::parse(value).is_none(), "{value}");
        assert_eq!(
            classify_anyhow(&metadata(value, None, 0).unwrap_err()),
            StreamFailure::Truncated,
            "{value}"
        );
    }
}

#[test]
fn range_payload_length_and_resource_length_are_distinct() {
    let range = metadata("bytes 5-9/10", Some(5), 5).unwrap();
    assert_eq!(range.total_bytes, Some(10));
    assert_eq!(range.range_bytes, Some(5));
    let unknown = metadata("bytes 5-9/*", Some(5), 5).unwrap();
    assert_eq!(unknown.total_bytes, None);
    assert_eq!(unknown.range_bytes, Some(5));
}

#[test]
fn range_start_must_match_the_requested_byte_even_on_the_first_request() {
    for position in [0, 4, 6, u64::MAX] {
        assert_eq!(
            classify_anyhow(&metadata("bytes 5-9/10", Some(5), position).unwrap_err()),
            StreamFailure::Truncated
        );
    }
}

#[test]
fn range_size_must_match_content_length_when_present() {
    assert!(metadata("bytes 5-9/10", None, 5).is_ok());
    for length in [0, 4, 6, u64::MAX] {
        assert_eq!(
            classify_anyhow(&metadata("bytes 5-9/10", Some(length), 5).unwrap_err()),
            StreamFailure::Truncated
        );
    }
}

#[test]
fn missing_or_non_text_content_range_is_rejected() {
    let mut headers = HeaderMap::new();
    assert_eq!(
        classify_anyhow(
            &ResponseMetadata::inspect(StatusCode::PARTIAL_CONTENT, &headers, Some(5), 0)
                .unwrap_err()
        ),
        StreamFailure::Truncated
    );
    headers.insert(
        CONTENT_RANGE,
        reqwest::header::HeaderValue::from_bytes(&[0xff]).unwrap(),
    );
    assert!(ResponseMetadata::inspect(StatusCode::PARTIAL_CONTENT, &headers, Some(5), 0).is_err());
}

#[test]
fn ordinary_response_uses_payload_length_and_preserves_mime_without_range_semantics() {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_RANGE, "bytes 0-4/999".parse().unwrap());
    headers.insert(CONTENT_TYPE, "audio/wav; codec=pcm".parse().unwrap());
    let response = ResponseMetadata::inspect(StatusCode::OK, &headers, Some(5), 0).unwrap();
    assert_eq!(response.total_bytes, Some(5));
    assert_eq!(response.range_bytes, None);
    assert_eq!(response.mime_type.as_deref(), Some("audio/wav; codec=pcm"));
    assert!(ResponseMetadata::inspect(StatusCode::OK, &headers, None, 0)
        .unwrap()
        .total_bytes
        .is_none());
}

#[test]
fn ignored_ranges_and_non_audio_success_statuses_keep_structured_errors() {
    let headers = HeaderMap::new();
    assert_eq!(
        classify_anyhow(
            &ResponseMetadata::inspect(StatusCode::OK, &headers, Some(5), 1).unwrap_err()
        ),
        StreamFailure::Truncated
    );
    for status in [201, 202, 204, 304, 404, 503] {
        let error =
            ResponseMetadata::inspect(StatusCode::from_u16(status).unwrap(), &headers, None, 0)
                .unwrap_err();
        assert_eq!(classify_anyhow(&error), StreamFailure::Http(status));
    }
}

#[test]
fn http_failure_status_does_not_reach_the_audio_decoder() {
    let (url, server) = serve(vec![(
        0,
        b"HTTP/1.1 503 Unavailable\r\nContent-Length: 4\r\nConnection: close\r\n\r\nbusy",
    )]);
    let error = match request_range(&client(), &url, 0) {
        Ok(_) => panic!("accepted a failed response"),
        Err(error) => error,
    };
    server.join().unwrap();
    assert_eq!(classify_anyhow(&error), StreamFailure::Http(503));
    assert!(error.to_string().contains("503"));
}

#[test]
fn non_audio_success_envelopes_preserve_server_errors_and_are_not_retryable() {
    let body =
        r#"{"subsonic-response":{"error":{"code":70,"message":"transcoding not configured"}}}"#;
    for mime in [
        "application/json",
        "APPLICATION/JSON; charset=utf-8",
        "text/html",
        "application/xml",
    ] {
        let response = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: {mime}\r\nConnection: close\r\n\r\n{body}", body.len()).into_bytes();
        let (url, server) = serve(vec![(0, response)]);
        let error = match request_range(&client(), &url, 0) {
            Ok(_) => panic!("accepted an error envelope with {mime}"),
            Err(error) => error,
        };
        server.join().unwrap();
        let kind = classify_anyhow(&error);
        assert_eq!(kind, StreamFailure::Http(200));
        assert!(!kind.is_retryable());
        assert!(error
            .to_string()
            .contains("transcoding not configured (Subsonic error 70)"));
    }
}

#[test]
fn bounded_error_body_read_keeps_the_existing_unicode_safe_summary() {
    let body = "音".repeat(4000);
    let response = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\n{body}", body.len()).into_bytes();
    let (url, server) = serve(vec![(0, response)]);
    let error = match request_range(&client(), &url, 0) {
        Ok(_) => panic!("accepted plain-text error content"),
        Err(error) => error,
    };
    server.join().unwrap();
    assert_eq!(classify_anyhow(&error), StreamFailure::Http(200));
    assert!(error
        .to_string()
        .ends_with(&format!("{}…", "音".repeat(200))));
}

#[test]
fn incomplete_error_body_keeps_received_diagnostics_and_server_classification() {
    let bytes = b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\ntranscoding unavailable";
    let (url, server) = serve(vec![(0, bytes)]);
    let error = match request_range(&client(), &url, 0) {
        Ok(_) => panic!("accepted an incomplete error response"),
        Err(error) => error,
    };
    server.join().unwrap();
    assert_eq!(classify_anyhow(&error), StreamFailure::Http(200));
    assert!(error.to_string().contains("transcoding unavailable"));
}

#[test]
fn endpoint_logging_removes_userinfo_query_parameters_and_fragments() {
    assert_eq!(
        stream_endpoint(
            "https://user:password@example.test:8443/rest/stream?id=song&t=token#secret"
        ),
        "https://example.test:8443/rest/stream"
    );
    assert_eq!(stream_endpoint("not a url"), "invalid-stream-url");
}

#[test]
fn connection_failure_diagnostics_do_not_expose_signed_stream_urls() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let url = format!("http://user:fake-password@{address}/audio?id=song&t=fake-token&s=fake-salt");
    let error = match request_range(&client(), &url, 0) {
        Ok(_) => panic!("connected to a closed fixture listener"),
        Err(error) => error,
    };
    assert_eq!(classify_anyhow(&error), StreamFailure::Connect);
    for secret in ["fake-password", "fake-token", "fake-salt"] {
        assert!(!error.to_string().contains(secret), "{error}");
    }
}

#[test]
fn empty_reads_do_not_consume_a_validated_response() {
    let bytes = b"HTTP/1.1 206 Partial Content\r\nContent-Length: 3\r\nContent-Range: bytes 2-4/5\r\nContent-Type: audio/mpeg\r\nConnection: close\r\n\r\ndio";
    let (url, server) = serve(vec![(2, bytes)]);
    let mut response = request_range(&client(), &url, 2).unwrap();
    assert_eq!(response.read(&mut []).unwrap(), 0);
    assert_eq!(response.len(), Some(5));
    assert_eq!(response.mime_type(), Some("audio/mpeg"));
    assert!(response.range_supported());
    let mut body = Vec::new();
    response.read_to_end(&mut body).unwrap();
    server.join().unwrap();
    assert_eq!(body, b"dio");
    assert_eq!(response.read(&mut [0]).unwrap(), 0);
}

#[test]
fn early_chunked_end_is_a_truncated_range_even_when_total_length_is_unknown() {
    let bytes = b"HTTP/1.1 206 Partial Content\r\nTransfer-Encoding: chunked\r\nContent-Range: bytes 0-2/*\r\nConnection: close\r\n\r\n2\r\nau\r\n0\r\n\r\n";
    let (url, server) = serve(vec![(0, bytes)]);
    let mut response = request_range(&client(), &url, 0).unwrap();
    assert_eq!(response.len(), None);
    let mut body = Vec::new();
    let error = response.read_to_end(&mut body).unwrap_err();
    server.join().unwrap();
    assert_eq!(body, b"au");
    assert_eq!(classify_io(&error), StreamFailure::Truncated);
    assert!(error.to_string().contains("declared byte range"));
}

#[test]
fn truncated_http_body_read_keeps_its_network_classification() {
    let bytes = b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\nConnection: close\r\n\r\nRIFF";
    let (url, server) = serve(vec![(0, bytes)]);
    let mut response = request_range(&client(), &url, 0).unwrap();
    let mut body = Vec::new();
    let error = response.read_to_end(&mut body).unwrap_err();
    server.join().unwrap();
    assert_eq!(body, b"RIFF");
    assert_eq!(classify_io(&error), StreamFailure::Truncated, "{error:?}");
}

#[test]
fn invalid_success_statuses_are_rejected_by_the_request_boundary() {
    for status in [201, 204] {
        let bytes = format!("HTTP/1.1 {status} Invalid Audio Response\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").into_bytes();
        let (url, server) = serve(vec![(0, bytes)]);
        let error = match request_range(&client(), &url, 0) {
            Ok(_) => panic!("accepted unexpected HTTP {status}"),
            Err(error) => error,
        };
        server.join().unwrap();
        assert_eq!(classify_anyhow(&error), StreamFailure::Http(status));
    }
}

#[test]
fn subsonic_error_envelope_yields_the_server_message() {
    // 标准错误信封：取出服务端给出的真实原因与错误码。
    let body = r#"{"subsonic-response":{"status":"failed","version":"1.16.1","error":{"code":0,"message":"transcoding not configured"}}}"#;
    assert_eq!(
        subsonic_error_detail(body),
        "transcoding not configured (Subsonic error 0)"
    );
    // 只有错误码：仍可给出可辨识的描述。
    assert_eq!(
        subsonic_error_detail(r#"{"error":{"code":70}}"#),
        "Subsonic error 70"
    );
    // JSON 但非信封结构：回退原始正文。
    let gateway = r#"{"detail":"gateway panic"}"#;
    assert_eq!(subsonic_error_detail(gateway), gateway);
}

#[test]
fn subsonic_error_envelope_falls_back_to_raw_text_when_unparseable() {
    // 非 JSON 正文：截断保留前 200 字符。
    let html = "<html>502 Bad Gateway</html>";
    assert_eq!(subsonic_error_detail(html), html);
    let long = "x".repeat(300);
    assert_eq!(
        subsonic_error_detail(&long),
        format!("{}…", "x".repeat(200))
    );
    // 空正文：给出占位描述而不是空字符串。
    assert_eq!(
        subsonic_error_detail(""),
        "the server returned an empty response"
    );
    assert_eq!(
        subsonic_error_detail("   "),
        "the server returned an empty response"
    );
}

#[test]
fn detects_transcoded_streams_without_logging_authentication_data() {
    assert!(is_transcoded_stream(
        "https://example.test/rest/stream.view?id=song-1&maxBitRate=320&t=secret"
    ));
    assert!(!is_transcoded_stream(
        "https://example.test/rest/stream.view?id=song-1&t=secret"
    ));
    assert!(!is_transcoded_stream("not a url"));
}
