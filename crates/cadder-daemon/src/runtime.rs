use crate::{
  logs::CaddyLogStore,
  paths::RuntimePaths,
  process_tree::ProcessTreeChild,
  runtime_file::{
    StagedRuntimeConfig, read_effective_config, remove_effective_config, restore_effective_config,
  },
};
use anyhow::{Context, Result};
use backon::{ExponentialBuilder, Retryable};
use cadder_ipc::{LogAttributionKind, LogSeverity, LogStreamIdentity, RuntimeState, RuntimeStatus};
#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};
use std::{
  path::{Path, PathBuf},
  process::Stdio,
  sync::{Arc, Mutex as StdMutex},
  time::Duration,
};
use tokio::{
  io::{AsyncBufReadExt, BufReader},
  sync::Mutex,
  task::yield_now,
  time::{Instant, timeout, timeout_at},
};
use tokio_util::task::TaskTracker;

use crate::caddy::RealCaddyResolver;

const CADDY_ADMIN_ENDPOINT: &str = "localhost:2019";
const MAX_RUNTIME_COMMAND_OUTPUT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone)]
pub enum CaddyRuntime {
  Real(Box<ProcessRuntime>),
  Mock(MockCaddyRuntime),
}

impl CaddyRuntime {
  pub fn real(resolver: RealCaddyResolver, paths: RuntimePaths) -> Self {
    Self::Real(Box::new(ProcessRuntime::new(resolver, paths)))
  }

  pub fn mock(paths: RuntimePaths) -> Self {
    Self::Mock(MockCaddyRuntime::new(paths))
  }

  pub async fn inspect(&self) -> RuntimeState {
    match self {
      Self::Real(runtime) => runtime.inspect().await,
      Self::Mock(runtime) => runtime.inspect().await,
    }
  }

  pub async fn apply_config(&self, rendered: &[u8], logs: &CaddyLogStore) -> Result<()> {
    let attempt = self.begin_apply_config(rendered, logs).await?;
    let (mut receipt, outcome) = attempt.into_parts();
    if let Err(error) = outcome {
      let rollback = receipt.rollback(logs).await;
      return match rollback {
        Ok(()) => Err(error),
        Err(rollback_error) => Err(error.context(format!(
          "uncertain runtime apply rollback also failed: {rollback_error:#}"
        ))),
      };
    }
    if let Err(error) = receipt.accept(logs).await {
      let rollback = receipt.rollback(logs).await;
      return match rollback {
        Ok(()) => Err(error),
        Err(rollback_error) => Err(error.context(format!(
          "runtime apply rollback also failed: {rollback_error:#}"
        ))),
      };
    }
    Ok(())
  }

  pub(crate) async fn begin_apply_config(
    &self,
    rendered: &[u8],
    logs: &CaddyLogStore,
  ) -> Result<RuntimeApplyAttempt> {
    match self {
      Self::Real(runtime) => {
        let (receipt, outcome) = runtime.begin_apply_config(rendered, logs).await?;
        Ok(RuntimeApplyAttempt {
          receipt: RuntimeApplyReceipt::Real(Box::new(receipt)),
          outcome,
        })
      }
      Self::Mock(runtime) => Ok(RuntimeApplyAttempt {
        receipt: RuntimeApplyReceipt::Mock(Box::new(runtime.begin_apply_config(rendered).await?)),
        outcome: Ok(()),
      }),
    }
  }

  pub async fn stop(&self) -> Result<()> {
    match self {
      Self::Real(runtime) => runtime.stop().await,
      Self::Mock(runtime) => runtime.stop().await,
    }
  }

  pub(crate) async fn stop_until(&self, deadline: Instant) -> RuntimeStopOutcome {
    match self {
      Self::Real(runtime) => runtime.stop_until(deadline).await,
      Self::Mock(runtime) => RuntimeStopOutcome::new(runtime.stop().await, true),
    }
  }

  pub(crate) async fn force_stop(&self) -> Result<()> {
    match self {
      Self::Real(runtime) => runtime.force_stop().await,
      Self::Mock(runtime) => runtime.stop().await,
    }
  }

  pub(crate) async fn begin_stop(&self, logs: &CaddyLogStore) -> Result<RuntimeStopAttempt> {
    match self {
      Self::Real(runtime) => {
        let (receipt, outcome) = runtime.begin_stop(logs).await?;
        Ok(RuntimeStopAttempt {
          receipt: RuntimeStopReceipt::Real(Box::new(receipt)),
          outcome,
        })
      }
      Self::Mock(runtime) => Ok(RuntimeStopAttempt {
        receipt: RuntimeStopReceipt::Mock(Box::new(runtime.begin_stop().await?)),
        outcome: Ok(()),
      }),
    }
  }
}

#[derive(Debug)]
pub(crate) struct RuntimeApplyAttempt {
  receipt: RuntimeApplyReceipt,
  outcome: Result<()>,
}

impl RuntimeApplyAttempt {
  pub(crate) fn into_parts(self) -> (RuntimeApplyReceipt, Result<()>) {
    (self.receipt, self.outcome)
  }
}

#[derive(Debug)]
pub(crate) struct RuntimeStopAttempt {
  receipt: RuntimeStopReceipt,
  outcome: Result<()>,
}

#[derive(Debug)]
pub(crate) struct RuntimeStopOutcome {
  result: Result<()>,
  quiescent: bool,
}

impl RuntimeStopOutcome {
  fn new(result: Result<()>, quiescent: bool) -> Self {
    Self { result, quiescent }
  }

  pub(crate) fn into_parts(self) -> (Result<()>, bool) {
    (self.result, self.quiescent)
  }
}

impl RuntimeStopAttempt {
  pub(crate) fn into_parts(self) -> (RuntimeStopReceipt, Result<()>) {
    (self.receipt, self.outcome)
  }
}

#[derive(Debug)]
pub(crate) enum RuntimeStopReceipt {
  Real(Box<ProcessRuntimeStopReceipt>),
  Mock(Box<MockRuntimeStopReceipt>),
}

impl RuntimeStopReceipt {
  pub(crate) fn accept(&mut self) -> Result<()> {
    match self {
      Self::Real(receipt) => receipt.accept(),
      Self::Mock(receipt) => receipt.accept(),
    }
  }

  pub(crate) async fn rollback(self, logs: &CaddyLogStore) -> Result<()> {
    match self {
      Self::Real(receipt) => receipt.rollback(logs).await,
      Self::Mock(receipt) => receipt.rollback().await,
    }
  }
}

#[derive(Debug)]
pub(crate) enum RuntimeApplyReceipt {
  Real(Box<ProcessRuntimeApplyReceipt>),
  Mock(Box<MockRuntimeApplyReceipt>),
}

impl RuntimeApplyReceipt {
  pub(crate) async fn accept(&mut self, logs: &CaddyLogStore) -> Result<()> {
    match self {
      Self::Real(receipt) => receipt.accept(),
      Self::Mock(receipt) => receipt.accept(logs).await,
    }
  }

  pub(crate) async fn rollback(self, logs: &CaddyLogStore) -> Result<()> {
    match self {
      Self::Real(receipt) => receipt.rollback(logs).await,
      Self::Mock(receipt) => receipt.rollback().await,
    }
  }
}

impl From<ProcessRuntime> for CaddyRuntime {
  fn from(runtime: ProcessRuntime) -> Self {
    Self::Real(Box::new(runtime))
  }
}

mod mock_runtime;
mod process;

pub use mock_runtime::MockCaddyRuntime;
use mock_runtime::{MockRuntimeApplyReceipt, MockRuntimeStopReceipt};
#[cfg(test)]
use process::*;
pub use process::{ProcessRuntime, RuntimeTimeouts};
use process::{ProcessRuntimeApplyReceipt, ProcessRuntimeStopReceipt};

#[cfg(test)]
mod tests;
