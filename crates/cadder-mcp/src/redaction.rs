use cadder_daemon::Redactor;
use std::path::{Path, PathBuf};

const MAX_FREE_TEXT_CHARS: usize = 400;

#[derive(Debug, Clone)]
pub struct McpRedactor {
  workspace_root: Option<PathBuf>,
  runtime_dir: PathBuf,
}

impl McpRedactor {
  pub fn new(workspace_root: Option<PathBuf>, runtime_dir: PathBuf) -> Self {
    Self {
      workspace_root,
      runtime_dir,
    }
  }

  pub fn redact_path(&self, raw: &str) -> String {
    if raw.trim().is_empty() {
      return raw.to_string();
    }

    let path = Path::new(raw);
    if !path.is_absolute() {
      return raw.replace('\\', "/");
    }

    if let Some(relative) = self.strip_prefix(path, self.workspace_root.as_deref()) {
      return relative;
    }

    if let Some(relative) = self.strip_prefix(path, Some(&self.runtime_dir)) {
      return format!("<runtime>/{}", relative);
    }

    path
      .file_name()
      .and_then(|name| name.to_str())
      .map(|name| format!("<redacted>/{name}"))
      .unwrap_or_else(|| "<redacted-path>".to_string())
  }

  pub fn redact_endpoint(&self, endpoint: Option<&str>) -> Option<String> {
    endpoint.map(|endpoint| {
      if endpoint.contains("127.0.0.1") || endpoint.contains("localhost") {
        "loopback".to_string()
      } else {
        "<redacted-endpoint>".to_string()
      }
    })
  }

  pub fn redact_text(&self, text: &str) -> String {
    let base = Redactor::redact(text);
    let sanitized = base
      .split_whitespace()
      .map(|token| self.redact_token(token))
      .collect::<Vec<_>>()
      .join(" ");
    truncate(&sanitized, MAX_FREE_TEXT_CHARS)
  }

  fn redact_token(&self, token: &str) -> String {
    let (prefix, core, suffix) = split_token(token);
    if core.is_empty() {
      return token.to_string();
    }

    let redacted = if looks_like_absolute_path(&core) {
      self.redact_path(&core)
    } else {
      core.to_string()
    };

    format!("{prefix}{redacted}{suffix}")
  }

  fn strip_prefix(&self, path: &Path, base: Option<&Path>) -> Option<String> {
    let base = base?;
    let relative = path.strip_prefix(base).ok()?;
    Some(relative.to_string_lossy().replace('\\', "/"))
  }
}

fn looks_like_absolute_path(token: &str) -> bool {
  let path = Path::new(token);
  path.is_absolute()
    || token.starts_with("\\\\")
    || token.starts_with('/')
    || token
      .as_bytes()
      .get(1)
      .is_some_and(|separator| *separator == b':')
}

fn split_token(token: &str) -> (String, String, String) {
  let bytes = token.as_bytes();
  let mut start = 0;
  while start < bytes.len() && is_wrapper(bytes[start] as char) {
    start += 1;
  }

  let mut end = bytes.len();
  while end > start && is_wrapper(bytes[end - 1] as char) {
    end -= 1;
  }

  (
    token[..start].to_string(),
    token[start..end].to_string(),
    token[end..].to_string(),
  )
}

fn is_wrapper(ch: char) -> bool {
  matches!(
    ch,
    '"' | '\'' | '`' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';' | '!' | '?'
  )
}

pub fn truncate(text: &str, max_chars: usize) -> String {
  let char_count = text.chars().count();
  if char_count <= max_chars {
    return text.to_string();
  }

  let truncated = text
    .chars()
    .take(max_chars.saturating_sub(3))
    .collect::<String>();
  format!("{truncated}...")
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn workspace_paths_become_relative() {
    let redactor = McpRedactor::new(
      Some(PathBuf::from("D:/Projects/Personal/Cadder")),
      PathBuf::from("D:/Users/test/runtime"),
    );

    assert_eq!(
      redactor.redact_path("D:/Projects/Personal/Cadder/crates/cadderctl/src/app.rs"),
      "crates/cadderctl/src/app.rs"
    );
  }

  #[test]
  fn runtime_paths_use_runtime_prefix() {
    let redactor = McpRedactor::new(
      Some(PathBuf::from("D:/Projects/Personal/Cadder")),
      PathBuf::from("D:/Users/test/runtime"),
    );

    assert_eq!(
      redactor.redact_path("D:/Users/test/runtime/daemon.json"),
      "<runtime>/daemon.json"
    );
  }

  #[test]
  fn free_text_redacts_secrets_and_paths() {
    let redactor = McpRedactor::new(
      Some(PathBuf::from("D:/Projects/Personal/Cadder")),
      PathBuf::from("D:/Users/test/runtime"),
    );

    let text = redactor
      .redact_text("Authorization: bearer token=abc D:/Projects/Personal/Cadder/Caddyfile failed");

    assert_eq!(text, "[redacted] [redacted] [redacted] Caddyfile failed");
  }
}
