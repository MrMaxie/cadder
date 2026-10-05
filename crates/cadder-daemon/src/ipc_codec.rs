//! Bounded newline-delimited JSON framing for every local IPC session.

use bytes::BytesMut;
use serde::Serialize;
use std::io::{self, Write};
use tokio_util::codec::{Decoder, Encoder, LinesCodec, LinesCodecError};

pub(crate) const MAX_IPC_FRAME_LENGTH: usize = 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub(crate) enum IpcCodecError {
  #[error("IPC frame exceeds the 1 MiB limit")]
  FrameTooLarge,
  #[error("IPC frame contains an embedded line delimiter")]
  EmbeddedDelimiter,
  #[error("IPC frame is not valid UTF-8")]
  InvalidUtf8(#[source] io::Error),
  #[error("IPC frame could not be serialized as JSON")]
  Serialization(#[source] serde_json::Error),
  #[error("IPC peer closed the connection before terminating the frame with LF")]
  UnterminatedFrame,
  #[error("IPC framing is unusable after an earlier protocol error")]
  Poisoned,
  #[error("IPC framing I/O failed")]
  Io(
    #[from]
    #[source]
    io::Error,
  ),
}

/// A one-line UTF-8 codec that fails closed after any framing violation.
#[derive(Debug)]
pub(crate) struct BoundedNdjsonCodec {
  lines: LinesCodec,
  checked_until: usize,
  poisoned: bool,
}

impl BoundedNdjsonCodec {
  pub(crate) fn new() -> Self {
    Self {
      lines: LinesCodec::new_with_max_length(MAX_IPC_FRAME_LENGTH),
      checked_until: 0,
      poisoned: false,
    }
  }

  fn fail<T>(&mut self, error: IpcCodecError) -> Result<T, IpcCodecError> {
    self.poisoned = true;
    Err(error)
  }

  fn map_lines_error<T>(&mut self, error: LinesCodecError) -> Result<T, IpcCodecError> {
    match error {
      LinesCodecError::MaxLineLengthExceeded => self.fail(IpcCodecError::FrameTooLarge),
      LinesCodecError::Io(error) if error.kind() == io::ErrorKind::InvalidData => {
        self.fail(IpcCodecError::InvalidUtf8(error))
      }
      LinesCodecError::Io(error) => self.fail(IpcCodecError::Io(error)),
    }
  }
}

impl Default for BoundedNdjsonCodec {
  fn default() -> Self {
    Self::new()
  }
}

pub(crate) fn encode_json_frame<T: Serialize>(value: &T) -> Result<Vec<u8>, IpcCodecError> {
  let mut output = BoundedFrameBuffer::new();
  if let Err(error) = serde_json::to_writer(&mut output, value) {
    if output.exceeded {
      return Err(IpcCodecError::FrameTooLarge);
    }
    return Err(IpcCodecError::Serialization(error));
  }
  output.bytes.push(b'\n');
  Ok(output.bytes)
}

struct BoundedFrameBuffer {
  bytes: Vec<u8>,
  exceeded: bool,
}

impl BoundedFrameBuffer {
  fn new() -> Self {
    Self {
      bytes: Vec::with_capacity(8 * 1024),
      exceeded: false,
    }
  }
}

impl Write for BoundedFrameBuffer {
  fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
    if bytes.len() > MAX_IPC_FRAME_LENGTH.saturating_sub(self.bytes.len()) {
      self.exceeded = true;
      return Err(io::Error::new(
        io::ErrorKind::FileTooLarge,
        "serialized IPC frame exceeds 1 MiB",
      ));
    }
    self.bytes.extend_from_slice(bytes);
    Ok(bytes.len())
  }

  fn flush(&mut self) -> io::Result<()> {
    Ok(())
  }
}

impl Decoder for BoundedNdjsonCodec {
  type Item = String;
  type Error = IpcCodecError;

  fn decode(&mut self, source: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
    if self.poisoned {
      return Err(IpcCodecError::Poisoned);
    }
    let scan_from = self.checked_until.min(source.len());
    let frame_end = source[scan_from..]
      .iter()
      .position(|byte| *byte == b'\n')
      .map_or(source.len(), |index| scan_from + index + 1);
    if source[scan_from..frame_end].contains(&b'\r') {
      return self.fail(IpcCodecError::EmbeddedDelimiter);
    }
    self.checked_until = frame_end;
    match self.lines.decode(source) {
      Ok(Some(frame)) => {
        self.checked_until = 0;
        Ok(Some(frame))
      }
      Ok(None) => Ok(None),
      Err(error) => self.map_lines_error(error),
    }
  }

  fn decode_eof(&mut self, source: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
    match self.decode(source)? {
      Some(frame) => Ok(Some(frame)),
      None if source.is_empty() => Ok(None),
      None => self.fail(IpcCodecError::UnterminatedFrame),
    }
  }
}

impl Encoder<String> for BoundedNdjsonCodec {
  type Error = IpcCodecError;

  fn encode(&mut self, frame: String, destination: &mut BytesMut) -> Result<(), Self::Error> {
    if self.poisoned {
      return Err(IpcCodecError::Poisoned);
    }
    if frame.len() > MAX_IPC_FRAME_LENGTH {
      return self.fail(IpcCodecError::FrameTooLarge);
    }
    if frame.bytes().any(|byte| matches!(byte, b'\r' | b'\n')) {
      return self.fail(IpcCodecError::EmbeddedDelimiter);
    }
    match self.lines.encode(frame, destination) {
      Ok(()) => Ok(()),
      Err(error) => self.map_lines_error(error),
    }
  }
}

#[cfg(test)]
mod tests;
