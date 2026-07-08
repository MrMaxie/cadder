mod body;
mod domains;
mod logs;
mod settings;

pub use domains::DomainsTab;
pub use logs::LogsTab;
pub use settings::SettingsTab;

pub(crate) use body::{TableBody, TableBodyRows};
