use serde::{Deserialize, Serialize};

use crate::GuiStateSnapshot;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StateChangeKind {
  Snapshot,
  RegistrationsChanged,
  RuntimeChanged,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StateChangedEvent {
  pub request_id: String,
  pub sequence_number: u64,
  pub change_kind: StateChangeKind,
  pub snapshot: GuiStateSnapshot,
  pub registration_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StateStreamHeartbeat {
  pub request_id: String,
  pub last_sequence_number: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StateStreamGap {
  pub request_id: String,
  pub first_missing_sequence_number: u64,
  pub last_missing_sequence_number: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateStreamRecord {
  Event(Box<StateChangedEvent>),
  Heartbeat(StateStreamHeartbeat),
  Gap(StateStreamGap),
}
