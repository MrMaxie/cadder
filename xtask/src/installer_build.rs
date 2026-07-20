fn runtime_installer(options: RuntimeInstallerOptions) -> Result<()> {
  verify_release_profile()?;
  if options.signing_mode == SigningMode::SignedRelease {
    verify_runtime_installer_signing_inputs(options.platform)?;
  }

  let work_dir = tempdir().context("create runtime installer work directory")?;
  let layout_dir = work_dir.path().join("runtime-layout");
  dist_with_builder(
    DistOptions {
      out_dir: layout_dir.clone(),
      target: options.target.clone(),
      topology: PortableTopology::Runtime,
    },
    build_release_binaries,
  )?;

  let payload_root = work_dir.path().join("payload-root");
  prepare_runtime_installer_payload(
    &layout_dir,
    &payload_root,
    options.target.as_deref(),
    options.platform,
  )?;

  remove_dir_if_exists(&options.out_dir)?;
  fs::create_dir_all(&options.out_dir)
    .with_context(|| format!("create {}", options.out_dir.display()))?;

  let artifacts = build_runtime_installer_artifacts(&payload_root, &layout_dir, &options)?;
  for artifact in &artifacts {
    let checksum_path = checksum_path_for(artifact)?;
    write_sha256_file(artifact, &checksum_path)?;
    write_runtime_installer_manifest(artifact, &options)?;
  }

  verify_runtime_installer_dist(&VerifyRuntimeInstallerDistOptions {
    dir: options.out_dir,
    version: options.version,
    platform: options.platform,
  })
}

fn prepare_runtime_installer_payload(
  layout_dir: &Path,
  payload_root: &Path,
  target: Option<&str>,
  platform: ReleasePlatform,
) -> Result<()> {
  remove_dir_if_exists(payload_root)?;
  match platform {
    ReleasePlatform::WindowsX64 => copy_dir_contents(
      layout_dir,
      &payload_root.join("ProgramFiles").join("Cadder"),
    ),
    ReleasePlatform::LinuxX64 | ReleasePlatform::MacosX64 | ReleasePlatform::MacosArm64 => {
      let bin_dir = payload_root.join("usr").join("local").join("bin");
      let config_dir = payload_root
        .join("usr")
        .join("local")
        .join("etc")
        .join("cadder");
      fs::create_dir_all(&bin_dir).with_context(|| format!("create {}", bin_dir.display()))?;
      fs::create_dir_all(&config_dir)
        .with_context(|| format!("create {}", config_dir.display()))?;
      for binary in RUNTIME_PORTABLE_BINARIES {
        let source = layout_dir.join(exe_name(binary, target));
        let target_path = bin_dir.join(binary);
        fs::copy(&source, &target_path)
          .with_context(|| format!("copy {} to {}", source.display(), target_path.display()))?;
      }
      fs::copy(
        layout_dir.join("cadder.toml"),
        config_dir.join("cadder.toml"),
      )
      .context("copy runtime installer sample config")?;
      Ok(())
    }
  }
}

fn build_runtime_installer_artifacts(
  payload_root: &Path,
  windows_layout_dir: &Path,
  options: &RuntimeInstallerOptions,
) -> Result<Vec<PathBuf>> {
  let mut artifacts = Vec::new();
  for kind in options.platform.required_runtime_installer_artifact_kinds() {
    let artifact = options.out_dir.join(runtime_installer_artifact_name(
      &options.version,
      options.platform,
      *kind,
    ));
    match kind {
      RuntimeInstallerArtifactKind::WindowsMsi => {
        build_windows_runtime_msi(windows_layout_dir, &artifact, options)?
      }
      RuntimeInstallerArtifactKind::LinuxDeb => {
        build_linux_runtime_deb(payload_root, &artifact, options)?
      }
      RuntimeInstallerArtifactKind::LinuxRpm => {
        build_linux_runtime_rpm(payload_root, &artifact, options)?
      }
      RuntimeInstallerArtifactKind::MacosPkg => {
        build_macos_runtime_pkg(payload_root, &artifact, options)?
      }
    }
    if !artifact.is_file() {
      bail!("{} did not create {}", kind.label(), artifact.display());
    }
    artifacts.push(artifact);
  }
  Ok(artifacts)
}

fn build_windows_runtime_msi(
  layout_dir: &Path,
  artifact: &Path,
  options: &RuntimeInstallerOptions,
) -> Result<()> {
  let wxs_path = artifact.with_extension("wxs");
  fs::write(
    &wxs_path,
    windows_runtime_wxs(layout_dir, &options.version, options.target.as_deref())?,
  )
  .with_context(|| format!("write {}", wxs_path.display()))?;
  let wxs = path_argument(&wxs_path)?;
  let output = path_argument(artifact)?;
  run("wix", &["build", &wxs, "-arch", "x64", "-o", &output])?;
  if options.signing_mode == SigningMode::SignedRelease {
    sign_windows_artifact(artifact)?;
  }
  fs::remove_file(&wxs_path).with_context(|| format!("remove {}", wxs_path.display()))?;
  Ok(())
}

fn windows_runtime_wxs(layout_dir: &Path, version: &str, target: Option<&str>) -> Result<String> {
  let mut components = String::new();
  let mut component_refs = String::new();
  for binary in RUNTIME_PORTABLE_BINARIES {
    let file_name = exe_name(binary, target);
    let source = xml_escape(&path_argument(&layout_dir.join(&file_name))?);
    let component_id = windows_installer_id(&format!("cmp_{file_name}"));
    let file_id = windows_installer_id(&format!("file_{file_name}"));
    components.push_str(&format!(
      r#"
        <Component Id="{component_id}" Guid="*">
          <File Id="{file_id}" Source="{source}" KeyPath="yes" />
        </Component>"#
    ));
    component_refs.push_str(&format!(r#"<ComponentRef Id="{component_id}" />"#));
  }
  let config_source = xml_escape(&path_argument(&layout_dir.join("cadder.toml"))?);
  components.push_str(&format!(
    r#"
        <Component Id="cmp_cadder_toml" Guid="*">
          <File Id="file_cadder_toml" Source="{config_source}" KeyPath="yes" />
        </Component>"#
  ));
  component_refs.push_str(r#"<ComponentRef Id="cmp_cadder_toml" />"#);

  Ok(format!(
    r#"<?xml version="1.0" encoding="UTF-8"?>
<Wix xmlns="http://wixtoolset.org/schemas/v4/wxs">
  <Package Name="{product}" Manufacturer="{manufacturer}" Version="{version}" UpgradeCode="{upgrade_code}" Scope="perMachine">
    <MajorUpgrade DowngradeErrorMessage="A newer version of {product} is already installed." />
    <MediaTemplate EmbedCab="yes" />
    <Feature Id="RuntimeFeature" Title="{product}" Level="1">
      <ComponentGroupRef Id="RuntimeComponents" />
    </Feature>
  </Package>
  <Fragment>
    <StandardDirectory Id="ProgramFiles64Folder">
      <Directory Id="INSTALLFOLDER" Name="Cadder">
{components}
      </Directory>
    </StandardDirectory>
  </Fragment>
  <Fragment>
    <ComponentGroup Id="RuntimeComponents">
      {component_refs}
    </ComponentGroup>
  </Fragment>
</Wix>
"#,
    product = RUNTIME_INSTALLER_PRODUCT_NAME,
    manufacturer = RUNTIME_INSTALLER_MANUFACTURER,
    upgrade_code = WINDOWS_RUNTIME_INSTALLER_UPGRADE_CODE,
  ))
}

fn build_linux_runtime_deb(
  payload_root: &Path,
  artifact: &Path,
  options: &RuntimeInstallerOptions,
) -> Result<()> {
  let control_dir = payload_root.join("DEBIAN");
  fs::create_dir_all(&control_dir).with_context(|| format!("create {}", control_dir.display()))?;
  let control = format!(
    "Package: {package}\nVersion: {version}\nSection: devel\nPriority: optional\nArchitecture: {arch}\nMaintainer: Cadder Maintainers <noreply@example.com>\nDescription: Cadder daemon-first runtime\n Cadder coordinates local Caddy reverse proxies through a per-user daemon, CLI, and PATH-facing Caddy shim.\n",
    package = RUNTIME_INSTALLER_PACKAGE_NAME,
    version = options.version,
    arch = options.platform.debian_architecture()?,
  );
  fs::write(control_dir.join("control"), control)
    .with_context(|| format!("write {}", control_dir.join("control").display()))?;
  let payload = path_argument(payload_root)?;
  let output = path_argument(artifact)?;
  run(
    "dpkg-deb",
    &["--build", "--root-owner-group", &payload, &output],
  )
}

fn build_linux_runtime_rpm(
  payload_root: &Path,
  artifact: &Path,
  options: &RuntimeInstallerOptions,
) -> Result<()> {
  let top_dir = artifact.with_extension("rpmbuild");
  remove_dir_if_exists(&top_dir)?;
  for name in ["BUILD", "BUILDROOT", "RPMS", "SOURCES", "SPECS", "SRPMS"] {
    fs::create_dir_all(top_dir.join(name))
      .with_context(|| format!("create {}", top_dir.join(name).display()))?;
  }
  let spec_path = top_dir.join("SPECS").join("cadder-runtime.spec");
  fs::write(
    &spec_path,
    linux_runtime_rpm_spec(payload_root, &options.version, options.platform)?,
  )
  .with_context(|| format!("write {}", spec_path.display()))?;
  let top = path_argument(&top_dir)?;
  let spec = path_argument(&spec_path)?;
  run(
    "rpmbuild",
    &[
      "-bb",
      "--define",
      &format!("_topdir {top}"),
      "--define",
      "_build_id_links none",
      &spec,
    ],
  )?;
  let built_rpm = top_dir
    .join("RPMS")
    .join(options.platform.rpm_architecture()?)
    .join(format!(
      "{package}-{version}-1.{arch}.rpm",
      package = RUNTIME_INSTALLER_PACKAGE_NAME,
      version = options.version,
      arch = options.platform.rpm_architecture()?,
    ));
  fs::copy(&built_rpm, artifact)
    .with_context(|| format!("copy {} to {}", built_rpm.display(), artifact.display()))?;
  remove_dir_if_exists(&top_dir)
}

fn linux_runtime_rpm_spec(
  payload_root: &Path,
  version: &str,
  platform: ReleasePlatform,
) -> Result<String> {
  let payload = path_argument(payload_root)?;
  Ok(format!(
    r#"Name: {package}
Version: {version}
Release: 1
Summary: Cadder daemon-first runtime
License: MIT
BuildArch: {arch}
AutoReqProv: no

%description
Cadder coordinates local Caddy reverse proxies through a per-user daemon, CLI,
and PATH-facing Caddy shim.

%prep

%build

%install
mkdir -p "%{{buildroot}}/usr/local/bin"
mkdir -p "%{{buildroot}}/usr/local/etc/cadder"
cp -a "{payload}/usr/local/bin/." "%{{buildroot}}/usr/local/bin/"
cp -a "{payload}/usr/local/etc/cadder/." "%{{buildroot}}/usr/local/etc/cadder/"

%files
/usr/local/bin/cadderd
/usr/local/bin/cadder
/usr/local/bin/caddy
/usr/local/etc/cadder/cadder.toml
"#,
    package = RUNTIME_INSTALLER_PACKAGE_NAME,
    arch = platform.rpm_architecture()?,
  ))
}

fn build_macos_runtime_pkg(
  payload_root: &Path,
  artifact: &Path,
  options: &RuntimeInstallerOptions,
) -> Result<()> {
  let payload = path_argument(payload_root)?;
  let output = path_argument(artifact)?;
  let mut args = vec![
    "--root".to_string(),
    payload,
    "--identifier".to_string(),
    RUNTIME_INSTALLER_IDENTIFIER.to_string(),
    "--version".to_string(),
    options.version.clone(),
    "--install-location".to_string(),
    "/".to_string(),
  ];
  if options.signing_mode == SigningMode::SignedRelease {
    args.push("--sign".to_string());
    args.push(required_env(MACOS_INSTALLER_SIGNING_IDENTITY_ENV)?);
  }
  args.push(output);
  let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
  run("pkgbuild", &refs)
}

fn sign_windows_artifact(artifact: &Path) -> Result<()> {
  let cert = required_env(WINDOWS_SIGNTOOL_CERT_PATH_ENV)?;
  let password = required_env(WINDOWS_SIGNTOOL_CERT_PASSWORD_ENV)?;
  let timestamp = env::var(WINDOWS_SIGNTOOL_TIMESTAMP_URL_ENV)
    .unwrap_or_else(|_| "http://timestamp.digicert.com".to_string());
  let artifact = path_argument(artifact)?;
  run(
    "signtool",
    &[
      "sign", "/fd", "sha256", "/td", "sha256", "/tr", &timestamp, "/f", &cert, "/p", &password,
      &artifact,
    ],
  )
}

fn verify_runtime_installer_signing_inputs(platform: ReleasePlatform) -> Result<()> {
  match platform {
    ReleasePlatform::WindowsX64 => {
      required_env(WINDOWS_SIGNTOOL_CERT_PATH_ENV)?;
      required_env(WINDOWS_SIGNTOOL_CERT_PASSWORD_ENV)?;
    }
    ReleasePlatform::MacosX64 | ReleasePlatform::MacosArm64 => {
      required_env(MACOS_INSTALLER_SIGNING_IDENTITY_ENV)?;
    }
    ReleasePlatform::LinuxX64 => {}
  }
  Ok(())
}

fn required_env(name: &str) -> Result<String> {
  let value = env::var(name).with_context(|| format!("{name} is required"))?;
  if value.trim().is_empty() {
    bail!("{name} must not be empty");
  }
  Ok(value)
}
