use super::*;

#[test]
fn ipc_codec_accepts_exact_limit_and_rejects_one_byte_more() {
  let mut codec = BoundedNdjsonCodec::new();
  let mut encoded = BytesMut::new();
  codec
    .encode("x".repeat(MAX_IPC_FRAME_LENGTH), &mut encoded)
    .unwrap();
  assert_eq!(encoded.len(), MAX_IPC_FRAME_LENGTH + 1);

  let mut codec = BoundedNdjsonCodec::new();
  let error = codec
    .encode("x".repeat(MAX_IPC_FRAME_LENGTH + 1), &mut BytesMut::new())
    .unwrap_err();
  assert!(matches!(error, IpcCodecError::FrameTooLarge));
  assert!(matches!(
    codec.encode("{}".to_string(), &mut BytesMut::new()),
    Err(IpcCodecError::Poisoned)
  ));
}

#[test]
fn ipc_codec_bounds_json_serialization_before_appending_lf() {
  let exact = "x".repeat(MAX_IPC_FRAME_LENGTH - 2);
  let encoded = encode_json_frame(&exact).unwrap();
  assert_eq!(encoded.len(), MAX_IPC_FRAME_LENGTH + 1);

  let oversized = "x".repeat(MAX_IPC_FRAME_LENGTH - 1);
  assert!(matches!(
    encode_json_frame(&oversized),
    Err(IpcCodecError::FrameTooLarge)
  ));
}

#[test]
fn ipc_codec_decoder_enforces_the_same_byte_limit() {
  let mut exact = BytesMut::from(vec![b'x'; MAX_IPC_FRAME_LENGTH].as_slice());
  exact.extend_from_slice(b"\n");
  assert_eq!(
    BoundedNdjsonCodec::new()
      .decode(&mut exact)
      .unwrap()
      .unwrap()
      .len(),
    MAX_IPC_FRAME_LENGTH
  );

  let mut oversized = BytesMut::from(vec![b'x'; MAX_IPC_FRAME_LENGTH + 1].as_slice());
  let mut codec = BoundedNdjsonCodec::new();
  assert!(matches!(
    codec.decode(&mut oversized),
    Err(IpcCodecError::FrameTooLarge)
  ));
  assert!(matches!(
    codec.decode(&mut BytesMut::from(&b"{}\n"[..])),
    Err(IpcCodecError::Poisoned)
  ));
}

#[test]
fn ipc_codec_rejects_unterminated_eof_and_embedded_delimiters() {
  let mut codec = BoundedNdjsonCodec::new();
  assert!(matches!(
    codec.decode_eof(&mut BytesMut::from(&b"{}"[..])),
    Err(IpcCodecError::UnterminatedFrame)
  ));

  let mut codec = BoundedNdjsonCodec::new();
  assert!(matches!(
    codec.encode("{}\n{}".to_string(), &mut BytesMut::new()),
    Err(IpcCodecError::EmbeddedDelimiter)
  ));

  let mut codec = BoundedNdjsonCodec::new();
  assert!(matches!(
    codec.decode(&mut BytesMut::from(&b"{}\r\n"[..])),
    Err(IpcCodecError::EmbeddedDelimiter)
  ));
}

#[test]
fn ipc_codec_delivers_a_valid_frame_before_rejecting_later_crlf() {
  let mut codec = BoundedNdjsonCodec::new();
  let mut input = BytesMut::from(&b"{}\n{}\r\n"[..]);

  assert_eq!(codec.decode(&mut input).unwrap().unwrap(), "{}");
  assert!(matches!(
    codec.decode(&mut input),
    Err(IpcCodecError::EmbeddedDelimiter)
  ));
}

#[test]
fn ipc_codec_rejects_invalid_utf8_and_poisoned_reuse() {
  let mut codec = BoundedNdjsonCodec::new();
  let mut invalid = BytesMut::from(&b"{\"value\":\"\xff\"}\n"[..]);

  assert!(matches!(
    codec.decode(&mut invalid),
    Err(IpcCodecError::InvalidUtf8(_))
  ));
  assert!(matches!(
    codec.decode(&mut BytesMut::from(&b"{}\n"[..])),
    Err(IpcCodecError::Poisoned)
  ));
}

#[test]
fn ipc_codec_handles_fragmented_utf8_frames() {
  let mut codec = BoundedNdjsonCodec::new();
  let mut input = BytesMut::from(&"{\"message\":\"zażółć\"}".as_bytes()[..12]);
  assert!(codec.decode(&mut input).unwrap().is_none());
  input.extend_from_slice(&"{\"message\":\"zażółć\"}".as_bytes()[12..]);
  input.extend_from_slice(b"\n");

  assert_eq!(
    codec.decode(&mut input).unwrap().unwrap(),
    "{\"message\":\"zażółć\"}"
  );
}
