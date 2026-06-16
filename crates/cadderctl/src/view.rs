use crate::cli::SeverityArg;
pub use cadder_operator::{
  ActionResultView, ConfigView, ConnectionStateView, CountsView, DaemonStartView, DaemonStatusView,
  DiagnosticsView, DomainListView, DomainResolveError, DomainView, EntrypointListView,
  EntrypointView, LogsView, RuntimeView, SelectedDomain, counts, daemon_status_connected,
  daemon_status_unavailable, diagnostics_view, domains_view, entrypoints_view, resolve_domain,
};
use cadder_protocol::LogSeverity;

pub fn map_severity(value: Option<SeverityArg>) -> Option<LogSeverity> {
  match value {
    None => None,
    Some(SeverityArg::Trace) => Some(LogSeverity::Trace),
    Some(SeverityArg::Debug) => Some(LogSeverity::Debug),
    Some(SeverityArg::Info) => Some(LogSeverity::Info),
    Some(SeverityArg::Warn) => Some(LogSeverity::Warn),
    Some(SeverityArg::Error) => Some(LogSeverity::Error),
    Some(SeverityArg::Fatal) => Some(LogSeverity::Fatal),
  }
}
