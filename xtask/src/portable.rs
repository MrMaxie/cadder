fn dist(options: DistOptions) -> Result<()> {
  verify_release_profile()?;
  dist_with_builder(options, build_release_binaries)
}

fn dist_with_builder(
  options: DistOptions,
  build_binaries: impl FnOnce(Option<&str>, PortableTopology) -> Result<()>,
) -> Result<()> {
  build_binaries(options.target.as_deref(), options.topology)?;
  prepare_dist_dir(&options.out_dir, options.target.as_deref())?;
  for binary in options.topology.binaries() {
    let source = release_binary_path(binary, options.target.as_deref());
    let target = options
      .out_dir
      .join(exe_name(binary, options.target.as_deref()));
    fs::copy(&source, &target)
      .with_context(|| format!("copy {} to {}", source.display(), target.display()))?;
  }
  if options.topology.includes_runtime() {
    fs::write(options.out_dir.join("cadder.toml"), SAMPLE_CADDER_TOML)
      .with_context(|| format!("write {}", options.out_dir.join("cadder.toml").display()))?;
  }

  verify_dist(&VerifyDistOptions {
    dir: options.out_dir,
    target: options.target,
    topology: options.topology,
  })
}

fn prepare_dist_dir(dir: &Path, target: Option<&str>) -> Result<()> {
  fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
  for binary in RUNTIME_PORTABLE_BINARIES {
    remove_file_if_exists(&dir.join(exe_name(binary, target)))?;
  }
  remove_file_if_exists(&dir.join("cadder.toml"))
}

fn remove_file_if_exists(path: &Path) -> Result<()> {
  match fs::metadata(path) {
    Ok(metadata) if metadata.is_file() => {
      fs::remove_file(path).with_context(|| format!("remove {}", path.display()))
    }
    Ok(_) => bail!(
      "expected removable portable file, found non-file {}",
      path.display()
    ),
    Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
    Err(error) => Err(error).with_context(|| format!("inspect {}", path.display())),
  }
}

fn verify_dist(options: &VerifyDistOptions) -> Result<()> {
  for binary in options.topology.binaries() {
    let path = portable_binary_path(options, binary);
    if !path.is_file() {
      bail!("portable binary missing: {}", path.display());
    }
  }
  if options.topology.includes_runtime() {
    let config = options.dir.join("cadder.toml");
    if !config.is_file() {
      bail!(
        "portable sample configuration missing: {}",
        config.display()
      );
    }
  }

  verify_dist_file_set(options)?;

  for binary in options.topology.binaries() {
    let path = portable_binary_path(options, binary);
    verify_portable_binary_command(binary, &path, "--help")?;
    verify_portable_binary_command(binary, &path, "--version")?;
  }

  if options.topology.includes_runtime() {
    let shim = portable_binary_path(options, "caddy");
    let stdout = run_portable_binary_command("caddy", &shim, "--cadder-shim-info")?;
    if !stdout.contains("\"role\":\"caddy-shim\"") && !stdout.contains("\"role\": \"caddy-shim\"") {
      bail!("caddy --cadder-shim-info did not report the Cadder shim role");
    }
  }

  let summary_label = format!("portable {} dist", options.topology.name());
  print_artifact_summary(&summary_label, &options.dir)?;

  Ok(())
}

fn verify_dist_file_set(options: &VerifyDistOptions) -> Result<()> {
  let mut expected = BTreeSet::new();
  for binary in options.topology.binaries() {
    expected.insert(exe_name(binary, options.target.as_deref()));
  }
  if options.topology.includes_runtime() {
    expected.insert("cadder.toml".to_string());
  }

  let mut actual = BTreeSet::new();
  for entry in sorted_dir_entries(&options.dir)? {
    if entry.file_type()?.is_file() {
      actual.insert(entry.file_name().to_string_lossy().to_string());
    }
  }

  if actual != expected {
    bail!(
      "portable dist {} contains unexpected file set: expected {:?}, found {:?}",
      options.dir.display(),
      expected,
      actual
    );
  }

  Ok(())
}

fn portable_binary_path(options: &VerifyDistOptions, binary: &str) -> PathBuf {
  options
    .dir
    .join(exe_name(binary, options.target.as_deref()))
}

fn verify_portable_binary_command(binary: &str, path: &Path, argument: &str) -> Result<()> {
  let stdout = run_portable_binary_command(binary, path, argument)?;
  if stdout.trim().is_empty() {
    bail!("{binary} {argument} produced empty stdout");
  }
  Ok(())
}

fn run_portable_binary_command(binary: &str, path: &Path, argument: &str) -> Result<String> {
  let mut command = Command::new(path);
  configure_hidden_child(&mut command);
  let output = command
    .arg(argument)
    .stdin(Stdio::null())
    .output()
    .with_context(|| format!("run {} {argument}", path.display()))?;
  if !output.status.success() {
    bail!("{binary} {argument} failed with {}", output.status);
  }
  Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn package(options: PackageOptions) -> Result<()> {
  verify_release_profile()?;
  package_with_dist(options, |dist_options| {
    dist_with_builder(dist_options, build_release_binaries)
  })
}

fn package_with_dist(
  options: PackageOptions,
  create_dist: impl FnOnce(DistOptions) -> Result<()>,
) -> Result<()> {
  let archive_stem = options
    .topology
    .package_archive_stem(&options.version, &options.platform)?;
  let layout_parent = options.out_dir.join("layouts");
  let layout_dir = layout_parent.join(&archive_stem);
  if layout_dir.exists() {
    fs::remove_dir_all(&layout_dir).with_context(|| format!("remove {}", layout_dir.display()))?;
  }

  create_dist(DistOptions {
    out_dir: layout_dir,
    target: options.target,
    topology: options.topology,
  })?;

  fs::create_dir_all(&options.out_dir)
    .with_context(|| format!("create {}", options.out_dir.display()))?;
  let archive_path = options.out_dir.join(format!(
    "{archive_stem}.{}",
    archive_extension(&options.platform)
  ));
  if archive_path.exists() {
    fs::remove_file(&archive_path).with_context(|| format!("remove {}", archive_path.display()))?;
  }

  match archive_kind(&options.platform) {
    ArchiveKind::Zip => write_zip_archive(&layout_parent, &archive_stem, &archive_path)?,
    ArchiveKind::TarGz => write_tar_gz_archive(&layout_parent, &archive_stem, &archive_path)?,
  }

  let checksum_path = options.out_dir.join(format!(
    "{}.sha256",
    archive_path
      .file_name()
      .and_then(|name| name.to_str())
      .context("archive path has no UTF-8 file name")?
  ));
  let checksum = write_sha256_file(&archive_path, &checksum_path)?;

  println!("created {}", archive_path.display());
  println!("created {}", checksum_path.display());
  println!("archive sha256 {checksum}");
  let summary_label = format!("portable {} package", options.topology.name());
  print_artifact_summary(&summary_label, &options.out_dir)?;
  Ok(())
}
