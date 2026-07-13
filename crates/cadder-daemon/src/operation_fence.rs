use std::sync::{Arc, Mutex};

use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum CommitRejection {
  #[error("the operation's commit permit was revoked")]
  Revoked,
  #[error("the operation belongs to an expired daemon lifecycle epoch")]
  StaleEpoch,
  #[error("the daemon is draining and does not accept mutations")]
  Draining,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LifecyclePhase {
  Active,
  Draining,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommitPermitState {
  Active,
  Finalized,
  Revoked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RevokeOutcome {
  Revoked,
  AlreadyRevoked,
  Finalized,
}

#[derive(Debug)]
struct LifecycleEpoch {
  number: u64,
  phase: LifecyclePhase,
  cancellation: CancellationToken,
}

#[derive(Debug, Clone)]
pub(crate) struct OperationFenceAuthority {
  lifecycle: Arc<Mutex<LifecycleEpoch>>,
}

impl Default for OperationFenceAuthority {
  fn default() -> Self {
    Self {
      lifecycle: Arc::new(Mutex::new(LifecycleEpoch {
        number: 0,
        phase: LifecyclePhase::Active,
        cancellation: CancellationToken::new(),
      })),
    }
  }
}

impl OperationFenceAuthority {
  pub(crate) fn issue(&self) -> Result<OperationFence, CommitRejection> {
    let lifecycle = self.lifecycle.lock().expect("lifecycle lock poisoned");
    if lifecycle.phase == LifecyclePhase::Draining {
      return Err(CommitRejection::Draining);
    }

    Ok(OperationFence {
      lifecycle: self.lifecycle.clone(),
      epoch: lifecycle.number,
      permit: Arc::new(Mutex::new(CommitPermitState::Active)),
      cancellation: lifecycle.cancellation.child_token(),
    })
  }

  pub(crate) fn begin_drain(&self) -> u64 {
    let mut lifecycle = self.lifecycle.lock().expect("lifecycle lock poisoned");
    if lifecycle.phase == LifecyclePhase::Draining {
      return lifecycle.number;
    }
    lifecycle.phase = LifecyclePhase::Draining;
    lifecycle.number = lifecycle
      .number
      .checked_add(1)
      .expect("daemon lifecycle epoch exhausted");
    lifecycle.cancellation.cancel();
    lifecycle.number
  }
}

#[derive(Debug, Clone)]
pub(crate) struct OperationFence {
  lifecycle: Arc<Mutex<LifecycleEpoch>>,
  epoch: u64,
  permit: Arc<Mutex<CommitPermitState>>,
  cancellation: CancellationToken,
}

impl OperationFence {
  pub(crate) fn cancellation(&self) -> CancellationToken {
    self.cancellation.clone()
  }

  pub(crate) fn revoke(&self) {
    let _ = self.try_revoke();
  }

  pub(crate) fn try_revoke(&self) -> RevokeOutcome {
    let mut permit = self.permit.lock().expect("operation permit lock poisoned");
    match *permit {
      CommitPermitState::Active => {
        *permit = CommitPermitState::Revoked;
        self.cancellation.cancel();
        RevokeOutcome::Revoked
      }
      CommitPermitState::Revoked => RevokeOutcome::AlreadyRevoked,
      CommitPermitState::Finalized => RevokeOutcome::Finalized,
    }
  }

  pub(crate) fn complete(&self) {
    let mut permit = self.permit.lock().expect("operation permit lock poisoned");
    if *permit == CommitPermitState::Active {
      *permit = CommitPermitState::Finalized;
    }
  }

  pub(crate) fn commit<T>(&self, commit: impl FnOnce() -> T) -> Result<T, CommitRejection> {
    let lifecycle = self.lifecycle.lock().expect("lifecycle lock poisoned");
    if lifecycle.number != self.epoch {
      return Err(CommitRejection::StaleEpoch);
    }
    if lifecycle.phase == LifecyclePhase::Draining {
      return Err(CommitRejection::Draining);
    }

    let permit = self.permit.lock().expect("operation permit lock poisoned");
    if *permit != CommitPermitState::Active {
      return Err(CommitRejection::Revoked);
    }

    Ok(commit())
  }

  pub(crate) fn commit_final<T>(&self, commit: impl FnOnce() -> T) -> Result<T, CommitRejection> {
    let lifecycle = self.lifecycle.lock().expect("lifecycle lock poisoned");
    if lifecycle.number != self.epoch {
      return Err(CommitRejection::StaleEpoch);
    }
    if lifecycle.phase == LifecyclePhase::Draining {
      return Err(CommitRejection::Draining);
    }

    let mut permit = self.permit.lock().expect("operation permit lock poisoned");
    if *permit != CommitPermitState::Active {
      return Err(CommitRejection::Revoked);
    }
    let result = commit();
    *permit = CommitPermitState::Finalized;
    Ok(result)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn operation_fence_revoke_invalidates_only_its_own_commit_permit() {
    let authority = OperationFenceAuthority::default();
    let revoked = authority.issue().unwrap();
    let independent = authority.issue().unwrap();

    revoked.revoke();

    assert_eq!(revoked.commit(|| ()).unwrap_err(), CommitRejection::Revoked);
    independent.commit(|| ()).unwrap();
    assert!(!independent.cancellation().is_cancelled());
  }

  #[test]
  fn operation_fence_drain_invalidates_old_work_and_rejects_new_work() {
    let authority = OperationFenceAuthority::default();
    let old = authority.issue().unwrap();

    assert_eq!(authority.begin_drain(), 1);

    assert!(old.cancellation().is_cancelled());
    assert_eq!(old.commit(|| ()).unwrap_err(), CommitRejection::StaleEpoch);
    assert_eq!(authority.issue().unwrap_err(), CommitRejection::Draining);
  }

  #[test]
  fn operation_fence_drain_is_idempotent() {
    let authority = OperationFenceAuthority::default();

    assert_eq!(authority.begin_drain(), 1);
    assert_eq!(authority.begin_drain(), 1);
    assert_eq!(authority.issue().unwrap_err(), CommitRejection::Draining);
  }

  #[test]
  fn operation_fence_commit_is_atomic_with_revoke() {
    let authority = OperationFenceAuthority::default();
    let fence = authority.issue().unwrap();
    let committed = Arc::new(Mutex::new(false));

    fence
      .commit(|| {
        *committed.lock().unwrap() = true;
      })
      .unwrap();
    fence.revoke();

    assert!(*committed.lock().unwrap());
    assert_eq!(fence.commit(|| ()).unwrap_err(), CommitRejection::Revoked);
  }

  #[test]
  fn operation_fence_final_commit_cannot_be_reclassified_as_timeout() {
    let authority = OperationFenceAuthority::default();
    let fence = authority.issue().unwrap();

    fence.commit_final(|| ()).unwrap();

    assert_eq!(fence.try_revoke(), RevokeOutcome::Finalized);
    assert_eq!(fence.commit(|| ()).unwrap_err(), CommitRejection::Revoked);
  }
}
