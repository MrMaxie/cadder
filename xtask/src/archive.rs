fn archive_kind(platform: &str) -> ArchiveKind {
  if platform.starts_with("windows") {
    ArchiveKind::Zip
  } else {
    ArchiveKind::TarGz
  }
}

fn archive_extension(platform: &str) -> &'static str {
  match archive_kind(platform) {
    ArchiveKind::Zip => "zip",
    ArchiveKind::TarGz => "tar.gz",
  }
}

fn write_zip_archive(layout_parent: &Path, root_dir: &str, archive_path: &Path) -> Result<()> {
  let file =
    File::create(archive_path).with_context(|| format!("create {}", archive_path.display()))?;
  let mut zip = ZipWriter::new(file);
  let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

  append_zip_dir(
    &mut zip,
    &layout_parent.join(root_dir),
    Path::new(root_dir),
    options,
  )?;
  zip
    .finish()
    .with_context(|| format!("finish {}", archive_path.display()))?;
  Ok(())
}

fn append_zip_dir(
  zip: &mut ZipWriter<File>,
  dir: &Path,
  archive_dir: &Path,
  options: SimpleFileOptions,
) -> Result<()> {
  zip
    .add_directory(path_to_archive_name(archive_dir)?, options)
    .with_context(|| format!("add ZIP directory {}", archive_dir.display()))?;

  for entry in sorted_dir_entries(dir)? {
    let entry_path = entry.path();
    let archive_path = archive_dir.join(entry.file_name());
    if entry_path.is_dir() {
      append_zip_dir(zip, &entry_path, &archive_path, options)?;
    } else {
      zip
        .start_file(path_to_archive_name(&archive_path)?, options)
        .with_context(|| format!("add ZIP file {}", archive_path.display()))?;
      let mut source =
        File::open(&entry_path).with_context(|| format!("open {}", entry_path.display()))?;
      io::copy(&mut source, zip)
        .with_context(|| format!("write ZIP file {}", archive_path.display()))?;
    }
  }

  Ok(())
}

fn write_tar_gz_archive(layout_parent: &Path, root_dir: &str, archive_path: &Path) -> Result<()> {
  let file =
    File::create(archive_path).with_context(|| format!("create {}", archive_path.display()))?;
  let encoder = GzEncoder::new(file, Compression::default());
  let mut tar = Builder::new(encoder);
  tar
    .append_dir_all(root_dir, layout_parent.join(root_dir))
    .with_context(|| {
      format!(
        "write TAR layout {}",
        layout_parent.join(root_dir).display()
      )
    })?;
  tar
    .into_inner()
    .context("finish TAR stream")?
    .finish()
    .with_context(|| format!("finish {}", archive_path.display()))?;
  Ok(())
}

fn write_sha256_file(archive_path: &Path, checksum_path: &Path) -> Result<String> {
  let file_name = archive_path
    .file_name()
    .and_then(|name| name.to_str())
    .context("archive path has no UTF-8 file name")?;
  let checksum = compute_sha256(archive_path)?;
  fs::write(checksum_path, format!("{checksum}  {file_name}\n"))
    .with_context(|| format!("write {}", checksum_path.display()))?;
  Ok(checksum)
}

fn checksum_path_for(artifact_path: &Path) -> Result<PathBuf> {
  let file_name = artifact_path
    .file_name()
    .and_then(|name| name.to_str())
    .context("artifact path has no UTF-8 file name")?;
  Ok(artifact_path.with_file_name(format!("{file_name}.sha256")))
}

fn verify_sha256_file(artifact_path: &Path) -> Result<()> {
  let checksum_path = checksum_path_for(artifact_path)?;
  let expected = compute_sha256(artifact_path)?;
  let file_name = artifact_path
    .file_name()
    .and_then(|name| name.to_str())
    .context("artifact path has no UTF-8 file name")?;
  let expected_line = format!("{expected}  {file_name}");
  let actual = fs::read_to_string(&checksum_path)
    .with_context(|| format!("read {}", checksum_path.display()))?;
  if actual.trim() != expected_line {
    bail!(
      "checksum file {} does not match {}",
      checksum_path.display(),
      artifact_path.display()
    );
  }
  Ok(())
}

fn compute_sha256(path: &Path) -> Result<String> {
  let mut file = File::open(path).with_context(|| format!("open {}", path.display()))?;
  let mut hasher = Sha256::new();
  let mut buffer = [0_u8; 8192];
  loop {
    let bytes_read = file
      .read(&mut buffer)
      .with_context(|| format!("read {}", path.display()))?;
    if bytes_read == 0 {
      break;
    }
    hasher.update(&buffer[..bytes_read]);
  }
  Ok(hex::encode(hasher.finalize()))
}

fn print_artifact_summary(label: &str, root: &Path) -> Result<()> {
  let mut paths = Vec::new();
  collect_file_paths(root, &mut paths)?;
  print_selected_artifact_summary(label, root, &paths)
}

fn collect_file_paths(dir: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
  for entry in sorted_dir_entries(dir)? {
    let path = entry.path();
    if path.is_dir() {
      collect_file_paths(&path, paths)?;
    } else {
      paths.push(path);
    }
  }
  paths.sort();
  Ok(())
}

fn print_selected_artifact_summary(label: &str, root: &Path, paths: &[PathBuf]) -> Result<()> {
  println!("{label} artifact summary:");
  if paths.is_empty() {
    println!("  no files");
    return Ok(());
  }

  for path in paths {
    let size = path_size(path)?;
    let display_path = relative_display_path(root, path);
    println!("  {display_path}\t{size} bytes");
  }
  Ok(())
}

fn path_size(path: &Path) -> Result<u64> {
  let metadata = fs::metadata(path).with_context(|| format!("inspect {}", path.display()))?;
  if metadata.is_file() {
    return Ok(metadata.len());
  }

  let mut size = 0_u64;
  collect_dir_size(path, &mut size)?;
  Ok(size)
}

fn collect_dir_size(dir: &Path, size: &mut u64) -> Result<()> {
  for entry in sorted_dir_entries(dir)? {
    let path = entry.path();
    let metadata = fs::metadata(&path).with_context(|| format!("inspect {}", path.display()))?;
    if metadata.is_dir() {
      collect_dir_size(&path, size)?;
    } else {
      *size = size.saturating_add(metadata.len());
    }
  }
  Ok(())
}

fn relative_display_path(root: &Path, path: &Path) -> String {
  path
    .strip_prefix(root)
    .unwrap_or(path)
    .display()
    .to_string()
}

fn path_to_archive_name(path: &Path) -> Result<String> {
  let mut parts = Vec::new();
  for component in path.components() {
    let component = component.as_os_str().to_str().with_context(|| {
      format!(
        "archive path contains non-UTF-8 component: {}",
        path.display()
      )
    })?;
    parts.push(component);
  }
  Ok(parts.join("/"))
}

fn path_argument(path: &Path) -> Result<String> {
  path
    .to_str()
    .map(ToOwned::to_owned)
    .with_context(|| format!("path contains non-UTF-8 data: {}", path.display()))
}

fn xml_escape(value: &str) -> String {
  value
    .replace('&', "&amp;")
    .replace('"', "&quot;")
    .replace('<', "&lt;")
    .replace('>', "&gt;")
}

fn windows_installer_id(value: &str) -> String {
  value
    .chars()
    .map(|character| {
      if character.is_ascii_alphanumeric() {
        character
      } else {
        '_'
      }
    })
    .collect()
}

fn sorted_dir_entries(dir: &Path) -> Result<Vec<fs::DirEntry>> {
  let mut entries = fs::read_dir(dir)
    .with_context(|| format!("read {}", dir.display()))?
    .collect::<std::result::Result<Vec<_>, _>>()
    .with_context(|| format!("read entries from {}", dir.display()))?;
  entries.sort_by_key(|entry| entry.file_name());
  Ok(entries)
}
