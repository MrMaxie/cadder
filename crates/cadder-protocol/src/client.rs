use crate::{IpcEnvelope, ProtocolResult, StateChangedEvent};

pub trait EnvelopeClient {
  fn request_envelope(&mut self, envelope: IpcEnvelope) -> ProtocolResult<IpcEnvelope>;
}

pub trait StateEventSubscription {
  fn next_state_event(&mut self) -> ProtocolResult<StateChangedEvent>;
}
