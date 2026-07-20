include!("openspec_check/core.rs");
include!("openspec_check/shared.rs");
include!("openspec_check/contracts.rs");
include!("openspec_check/implementation.rs");
include!("openspec_check/tasks.rs");
include!("openspec_check/content.rs");
include!("openspec_check/parsing.rs");

#[cfg(test)]
mod tests {
  use super::*;
  use tempfile::tempdir;

  include!("openspec_check/tests.rs");
}
