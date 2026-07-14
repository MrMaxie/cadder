mod body;
mod domains;
mod logs;
mod settings;
mod status;

pub use domains::DomainsTab;
pub use logs::LogsTab;
pub use settings::SettingsTab;
pub use status::StatusTab;

pub(crate) use body::{TableBody, TableBodyRows};
