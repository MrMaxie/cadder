use std::{
  env,
  fs::{self, OpenOptions},
  io::{self, Write},
  path::{Path, PathBuf},
  process::{Command, ExitCode, Stdio},
  thread,
  time::Duration,
};

const ADAPTER_CONFIG: &str = r#"{"apps":{"http":{"servers":{"srv0":{"routes":[{"match":[{"host":["adapter.localhost"]}],"handle":[{"handler":"static_response","body":"ok"}],"terminal":true}]}}}}}"#;
const PROJECT_CONFIG: &str = r#"{"apps":{"http":{"servers":{"srv0":{"routes":[{"match":[{"host":["project.localhost"]}],"handle":[{"handler":"static_response","body":"ok"}],"terminal":true}]}}}}}"#;

fn main() -> ExitCode {
  match run() {
    Ok(code) => ExitCode::from(code),
    Err(error) => {
      eprintln!("Cadder native test process failed: {error}");
      ExitCode::from(70)
    }
  }
}

fn run() -> io::Result<u8> {
  let executable = env::current_exe()?;
  let directory = executable
    .parent()
    .ok_or_else(|| io::Error::other("test process executable has no parent directory"))?;
  let mode = env::var("CADDER_TEST_PROCESS_MODE")
    .or_else(|_| fs::read_to_string(directory.join("cadder-test.mode")))?;
  let mode = mode.trim();

  match mode {
    "process-exit-success" => Ok(0),
    "process-block" => process_block(directory),
    "process-orphan-parent" => process_orphan_parent(&executable, directory),
    "process-sleep" => process_sleep(),
    "docker-proxy" => docker_proxy(directory),
    _ => fake_caddy(directory, mode),
  }
}

fn process_block(directory: &Path) -> io::Result<u8> {
  fs::write(directory.join("process-tree.started"), b"started")?;
  process_sleep()
}

fn process_orphan_parent(executable: &Path, directory: &Path) -> io::Result<u8> {
  let child = Command::new(executable)
    .env("CADDER_TEST_PROCESS_MODE", "process-sleep")
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()?;
  fs::write(
    directory.join("process-tree.grandchild-started"),
    child.id().to_string(),
  )?;
  Ok(0)
}

fn process_sleep() -> io::Result<u8> {
  thread::sleep(Duration::from_secs(60));
  Ok(0)
}

fn fake_caddy(directory: &Path, mode: &str) -> io::Result<u8> {
  let arguments = env::args().skip(1).collect::<Vec<_>>();
  append_line(directory.join("fake-caddy.log"), &arguments.join(" "))?;

  match arguments.first().map(String::as_str) {
    Some("adapt") => adapt(mode),
    Some("reload") => reload(directory, mode),
    Some("stop") => stop(directory, mode),
    Some("run") => run_server(directory, mode, &arguments),
    _ => Ok(64),
  }
}

fn adapt(mode: &str) -> io::Result<u8> {
  match mode {
    "adapter-fail-adapt" | "project-adapt-fail" => {
      eprintln!("adapt failed");
      Ok(if mode == "adapter-fail-adapt" { 6 } else { 7 })
    }
    "adapter-invalid-adapt-json" | "project-adapt-invalid" => {
      println!("not-json");
      Ok(0)
    }
    "adapter-slow-adapt" => {
      thread::sleep(Duration::from_millis(500));
      println!("{{\"apps\":{{}}}}");
      Ok(0)
    }
    "project-adapt-slow" => {
      thread::sleep(Duration::from_secs(60));
      println!("{{\"apps\":{{}}}}");
      Ok(0)
    }
    mode if mode.starts_with("adapter-") => {
      println!("{ADAPTER_CONFIG}");
      Ok(0)
    }
    mode if mode.starts_with("project-") => {
      println!("{PROJECT_CONFIG}");
      Ok(0)
    }
    _ => Ok(1),
  }
}

fn reload(directory: &Path, mode: &str) -> io::Result<u8> {
  if mode == "runtime-never-ready" {
    eprintln!("runtime not ready");
    return Ok(7);
  }
  if mode == "runtime-delayed-config-read" {
    thread::sleep(Duration::from_millis(500));
    return Ok(if directory.join("fake-caddy.config-read").is_file() {
      0
    } else {
      7
    });
  }
  if matches!(mode, "adapter-fail-reload" | "runtime-fail-reload") {
    let first_reload = directory.join("fake-caddy.first-reload");
    if first_reload.is_file() {
      eprintln!("reload failed");
      return Ok(7);
    }
    fs::write(first_reload, b"ready")?;
  }
  Ok(0)
}

fn stop(directory: &Path, mode: &str) -> io::Result<u8> {
  if matches!(mode, "runtime-slow-stop" | "project-runtime-slow-stop") {
    thread::sleep(Duration::from_secs(6));
  }
  fs::write(directory.join("fake-caddy.stop"), b"stop")?;
  Ok(if mode == "runtime-fail-stop" { 7 } else { 0 })
}

fn run_server(directory: &Path, mode: &str, arguments: &[String]) -> io::Result<u8> {
  if mode == "runtime-fail-run" {
    eprintln!("run failed");
    return Ok(7);
  }
  println!("fake runtime started");

  if mode == "runtime-delayed-config-read" {
    thread::sleep(Duration::from_millis(350));
    let config = argument_value(arguments, "--config")
      .map(PathBuf::from)
      .filter(|path| path.is_file());
    if config.is_none() {
      eprintln!("config missing");
      return Ok(9);
    }
    append_line(directory.join("fake-caddy.log"), "config-read")?;
    fs::write(directory.join("fake-caddy.config-read"), b"ready")?;
  }

  let stop = directory.join("fake-caddy.stop");
  let exit = directory.join("fake-caddy.exit");
  loop {
    if stop.is_file() {
      return Ok(0);
    }
    if mode == "runtime-short-run" && exit.is_file() {
      fs::remove_file(exit)?;
      append_line(directory.join("fake-caddy.log"), "run-exited")?;
      return Ok(0);
    }
    thread::sleep(Duration::from_millis(10));
  }
}

fn docker_proxy(directory: &Path) -> io::Result<u8> {
  let container_id = read_trimmed(directory.join("docker-container-id"))?;
  let host_root = PathBuf::from(read_trimmed(directory.join("docker-host-root"))?);
  let canonical_root = PathBuf::from(read_trimmed(directory.join("docker-canonical-root"))?);
  let container_root = read_trimmed(directory.join("docker-container-root"))?;
  let translated = env::args()
    .skip(1)
    .map(|argument| translate_path(&argument, [&host_root, &canonical_root], &container_root))
    .collect::<Vec<_>>();
  let log_path = PathBuf::from(read_trimmed(directory.join("docker-log-path"))?);
  append_line(log_path, &translated.join(" "))?;
  let status = Command::new("docker")
    .arg("exec")
    .arg(container_id)
    .arg("caddy")
    .args(&translated)
    .status()?;
  Ok(
    status
      .code()
      .and_then(|code| u8::try_from(code).ok())
      .unwrap_or(1),
  )
}

fn translate_path(argument: &str, host_roots: [&Path; 2], container_root: &str) -> String {
  for root in host_roots {
    if Path::new(argument) == root {
      return container_root.to_string();
    }
    if let Ok(relative) = Path::new(argument).strip_prefix(root) {
      let relative = relative.to_string_lossy().replace('\\', "/");
      return format!("{container_root}/{relative}");
    }
  }
  argument.to_string()
}

fn argument_value<'a>(arguments: &'a [String], name: &str) -> Option<&'a str> {
  arguments
    .windows(2)
    .find(|pair| pair[0] == name)
    .map(|pair| pair[1].as_str())
}

fn read_trimmed(path: PathBuf) -> io::Result<String> {
  Ok(fs::read_to_string(path)?.trim().to_string())
}

fn append_line(path: PathBuf, line: &str) -> io::Result<()> {
  let mut file = OpenOptions::new().create(true).append(true).open(path)?;
  writeln!(file, "{line}")
}
