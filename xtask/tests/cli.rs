use std::{
  env, fs,
  path::{Path, PathBuf},
  process::{Command, Output},
};

include!("cli/basic.rs");
include!("cli/validation.rs");
include!("cli/runtime.rs");
include!("cli/release.rs");
include!("cli/errors.rs");
include!("cli/support.rs");
