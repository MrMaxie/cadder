  #[test]
  fn parser_accepts_valid_main_requirement() {
    let (requirements, diagnostics) = parse_spec_text(
      Path::new("spec.md"),
      "local-control-plane",
      "IPC-",
      true,
      "### Requirement: IPC-001: Owner access\nCadder MUST authenticate the owner.\n\n#### Scenario: Owner connects\n- **WHEN** the owner connects\n- **THEN** access is granted\n",
    );

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(requirements.len(), 1);
    assert_eq!(requirements[0].id, "IPC-001");
    assert_eq!(requirements[0].operation, RequirementOperation::Main);
  }

  #[test]
  fn parser_accepts_valid_added_requirement() {
    let (requirements, diagnostics) = parse_spec_text(
      Path::new("spec.md"),
      "caddy-runtime",
      "CAD-",
      false,
      "## ADDED Requirements\n\n### Requirement: CAD-001: Trusted executable\nCadder SHALL use a trusted executable.\n\n#### Scenario: Trusted path\n- **WHEN** resolution succeeds\n- **THEN** the path is pinned\n",
    );

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(requirements[0].operation, RequirementOperation::Added);
  }

  #[test]
  fn parser_reports_wrong_prefix() {
    let (_, diagnostics) = parse_spec_text(
      Path::new("spec.md"),
      "caddy-runtime",
      "CAD-",
      true,
      "### Requirement: IPC-001: Wrong prefix\nCadder MUST reject it.\n\n#### Scenario: Invalid\n- **WHEN** parsed\n- **THEN** validation fails\n",
    );

    assert!(
      diagnostics[0]
        .message
        .contains("does not use capability prefix")
    );
  }

  #[test]
  fn parser_reports_missing_normative_text_and_scenario() {
    let (_, diagnostics) = parse_spec_text(
      Path::new("spec.md"),
      "runtime-storage",
      "STO-",
      true,
      "### Requirement: STO-001: Durable state\nThe database stores state.\n",
    );

    assert_eq!(diagnostics.len(), 2);
    assert!(
      diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("SHALL or MUST"))
    );
    assert!(
      diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("at least one scenario"))
    );
  }

  #[test]
  fn parser_accepts_removed_requirements_without_scenarios() {
    let (requirements, diagnostics) = parse_spec_text(
      Path::new("spec.md"),
      "caddy-runtime",
      "CAD-",
      false,
      "## REMOVED Requirements\n\n### Requirement: CAD-001: Retired contract\n\n**Reason**: The contract is replaced.\n\n**Migration**: Use CAD-002.\n",
    );

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(requirements[0].operation, RequirementOperation::Removed);
  }

  #[test]
  fn parser_accepts_renames_that_retain_the_stable_id() {
    let (requirements, diagnostics) = parse_spec_text(
      Path::new("spec.md"),
      "caddy-runtime",
      "CAD-",
      false,
      "## RENAMED Requirements\n\n- FROM: `### Requirement: CAD-001: Old title`\n- TO: `### Requirement: CAD-001: New title`\n",
    );

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(requirements[0].id, "CAD-001");
    assert_eq!(requirements[0].operation, RequirementOperation::Renamed);
  }

  #[test]
  fn requirement_id_rejects_zero_sequence_and_lowercase_text() {
    assert!(!is_requirement_id("IPC-000"));
    assert!(!is_requirement_id("ipc-001"));
    assert!(is_requirement_id("IPC-001"));
    assert!(extract_known_requirement_ids("XIPC-001Y").is_empty());
    assert!(extract_known_requirement_ids("xIPC-001y").is_empty());
    assert!(extract_task_requirement_ids("Implement IPC-001").is_empty());
    assert_eq!(
      extract_task_requirement_ids("Implement [IPC-001]"),
      BTreeSet::from(["IPC-001".to_string()])
    );
  }

  #[test]
  fn repository_accepts_a_valid_contract_change() {
    let dir = tempdir().unwrap();
    write_main_specs_except(dir.path(), "caddy-runtime");
    write_file(
      dir.path(),
      "openspec/changes/add-runtime/.openspec.yaml",
      "schema: spec-driven\n",
    );
    write_file(
      dir.path(),
      "openspec/changes/add-runtime/proposal.md",
      "## Capabilities\n\n### New Capabilities\n\n- `caddy-runtime`: Runtime contract.\n\n### Modified Capabilities\n\nNone.\n",
    );
    write_file(
      dir.path(),
      "openspec/changes/add-runtime/specs/caddy-runtime/spec.md",
      valid_delta_spec("CAD-001"),
    );

    validate_repository(dir.path()).unwrap();
  }

  #[test]
  fn repository_reports_added_collisions_and_dangling_modifications() {
    let dir = tempdir().unwrap();
    write_complete_main_specs(dir.path());
    write_file(
      dir.path(),
      "openspec/changes/change-runtime/.openspec.yaml",
      "schema: spec-driven\n",
    );
    write_file(
      dir.path(),
      "openspec/changes/change-runtime/proposal.md",
      "## Capabilities\n\n### New capabilities\n\n- `caddy-runtime`: Runtime contract.\n",
    );
    write_file(
      dir.path(),
      "openspec/changes/change-runtime/specs/caddy-runtime/spec.md",
      "## ADDED Requirements\n\n### Requirement: CAD-001: Duplicate\nCadder MUST reject duplicates.\n\n#### Scenario: Duplicate\n- **WHEN** validation runs\n- **THEN** it fails\n\n## MODIFIED Requirements\n\n### Requirement: CAD-002: Missing\nCadder MUST reject dangling changes.\n\n#### Scenario: Missing\n- **WHEN** validation runs\n- **THEN** it fails\n",
    );

    let error = validate_repository(dir.path()).unwrap_err().to_string();
    assert!(error.contains("already exists in main specs"), "{error}");
    assert!(error.contains("does not exist in main specs"), "{error}");
  }

  #[test]
  fn implementation_verification_follows_completed_tasks() {
    let dir = tempdir().unwrap();
    write_complete_main_specs(dir.path());
    let change = "openspec/changes/secure-ipc";
    write_file(
      dir.path(),
      &format!("{change}/.openspec.yaml"),
      "schema: implementation\n",
    );
    write_file(
      dir.path(),
      &format!("{change}/proposal.md"),
      "## Requirement IDs\n\n- `IPC-001`: Owner authentication\n",
    );
    write_file(
      dir.path(),
      &format!("{change}/design.md"),
      "## Contract\n\n`IPC-001` uses peer identity.\n",
    );
    write_file(
      dir.path(),
      &format!("{change}/tasks.md"),
      "## 1. Implementation\n\n- [ ] 1.1 `[IPC-001]` Authenticate peers. (verification: focused peer-auth test)\n",
    );
    validate_repository(dir.path()).unwrap();

    write_file(
      dir.path(),
      &format!("{change}/verification.md"),
      valid_verification(),
    );
    let early = validate_repository(dir.path()).unwrap_err().to_string();
    assert!(early.contains("must not exist before"), "{early}");

    write_file(
      dir.path(),
      &format!("{change}/tasks.md"),
      "## 1. Implementation\n\n- [x] 1.1 `[IPC-001]` Authenticate peers. (verification: focused peer-auth test)\n",
    );
    validate_repository(dir.path()).unwrap();
  }

  #[test]
  fn implementation_reports_task_scope_and_evidence_errors() {
    let dir = tempdir().unwrap();
    write_complete_main_specs(dir.path());
    let change = "openspec/changes/secure-ipc";
    write_file(
      dir.path(),
      &format!("{change}/.openspec.yaml"),
      "schema: implementation\n",
    );
    write_file(
      dir.path(),
      &format!("{change}/proposal.md"),
      "## Requirement IDs\n\n- `IPC-001`: Owner authentication\n",
    );
    write_file(
      dir.path(),
      &format!("{change}/design.md"),
      "`IPC-001` uses peer identity.\n",
    );
    write_file(
      dir.path(),
      &format!("{change}/tasks.md"),
      "- [x] 1.1 `[CAD-001]` Wrong scope. (verification: TODO)\n- [x] 1.1 Missing ID. (verification: focused test)\n",
    );
    write_file(
      dir.path(),
      &format!("{change}/verification.md"),
      valid_verification(),
    );

    let error = validate_repository(dir.path()).unwrap_err().to_string();
    assert!(
      error.contains("out-of-scope requirement `CAD-001`"),
      "{error}"
    );
    assert!(error.contains("task number `1.1` is duplicated"), "{error}");
    assert!(error.contains("must reference at least one"), "{error}");
    assert!(error.contains("must name a concrete"), "{error}");
  }

  #[test]
  fn content_boundaries_allow_policy_text_and_reject_private_paths() {
    let root = Path::new("D:/Projects/Personal/Cadder");
    assert!(!has_forbidden_local_reference(
      "Pages MUST NOT expose `.local` data."
    ));
    assert!(has_forbidden_local_reference("Read `.local/notes.md`."));
    assert!(has_forbidden_local_reference("Keep using `.local` notes."));
    assert!(!has_forbidden_local_reference("Use app.localhost."));
    assert!(has_personal_absolute_path(
      "Use D:\\Projects\\Personal\\Cadder\\target.",
      root
    ));
    assert!(has_personal_absolute_path("Use /home/alex/cadder.", root));
    assert!(!has_personal_absolute_path(
      "Install to C:\\Program Files\\Cadder.",
      root
    ));
    assert!(!has_personal_absolute_path(
      "Install to D:\\Apps\\Cadder.",
      root
    ));
  }

  #[test]
  fn shared_config_rules_reject_schema_specific_artifacts() {
    let dir = tempdir().unwrap();
    write_file(
      dir.path(),
      "openspec/config.yaml",
      "schema: spec-driven\n\nrules:\n  proposal:\n    - Shared rule.\n  specs:\n    - Contract-only rule.\n",
    );
    let mut diagnostics = Vec::new();

    validate_shared_artifact_rules(&dir.path().join("openspec"), &mut diagnostics);

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].line, 6);
    assert!(
      diagnostics[0]
        .message
        .contains("not shared by the spec-driven and implementation schemas")
    );
  }

  #[test]
  fn change_classification_skips_archive_and_rejects_duplicate_schema_fields() {
    let dir = tempdir().unwrap();
    write_file(
      dir.path(),
      "openspec/changes/current/.openspec.yaml",
      "schema: spec-driven\n",
    );
    write_file(
      dir.path(),
      "openspec/changes/archive/old/.openspec.yaml",
      "schema: spec-driven\n",
    );
    assert_eq!(spec_driven_change_names(dir.path()).unwrap(), ["current"]);

    write_file(
      dir.path(),
      "openspec/changes/current/.openspec.yaml",
      "schema: spec-driven\nschema: implementation\n",
    );
    let error = spec_driven_change_names(dir.path())
      .unwrap_err()
      .to_string();
    assert!(error.contains("exactly one"), "{error}");
  }

  #[test]
  fn repository_requires_every_accepted_capability() {
    let dir = tempdir().unwrap();
    write_complete_main_specs(dir.path());
    fs::remove_file(
      dir
        .path()
        .join("openspec/specs/documentation-experience/spec.md"),
    )
    .unwrap();

    let error = validate_repository(dir.path()).unwrap_err().to_string();
    assert!(
      error.contains("missing capability `documentation-experience`"),
      "{error}"
    );
  }

  #[test]
  fn repository_rejects_generated_main_spec_purpose_placeholders() {
    let dir = tempdir().unwrap();
    write_complete_main_specs(dir.path());
    write_file(
      dir.path(),
      "openspec/specs/caddy-runtime/spec.md",
      "# caddy-runtime Specification\n\n## Purpose\nTBD - created by archiving change add-runtime. Update Purpose after archive.\n\n## Requirements\n\n### Requirement: CAD-001: Accepted contract\nCadder MUST satisfy the contract.\n\n#### Scenario: Contract holds\n- **WHEN** Cadder operates\n- **THEN** the contract holds\n",
    );

    let error = validate_repository(dir.path()).unwrap_err().to_string();
    assert!(
      error.contains("concrete product-level description"),
      "{error}"
    );
  }

  #[test]
  fn verification_requires_the_evidence_section_and_every_named_gate() {
    let path = Path::new("verification.md");
    let requirements = BTreeSet::from(["IPC-001".to_string()]);
    let tasks = [ImplementationTask {
      number: "1.1".to_string(),
      complete: true,
      requirement_ids: requirements.clone(),
    }];
    let mut diagnostics = Vec::new();
    validate_verification(
      path,
      "## Notes\n\n| `IPC-001` | `1.1` | `cargo test` | Pass |\n\n## Gate\n\n- [x Focused tests pass.\n",
      &requirements,
      &tasks,
      &mut diagnostics,
    );

    assert!(
      diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("`## Evidence`")),
      "{diagnostics:?}"
    );
    assert!(
      diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("exact `- [x] Description`")),
      "{diagnostics:?}"
    );
    assert!(
      diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("missing completed gate")),
      "{diagnostics:?}"
    );
  }

  fn write_main_spec(root: &Path, capability: &str, id: &str) {
    write_file(
      root,
      &format!("openspec/specs/{capability}/spec.md"),
      format!(
        "# {capability} Specification\n\n## Purpose\nDefine the accepted Cadder behavior for the {capability} capability and its observable product boundaries.\n\n## Requirements\n\n### Requirement: {id}: Accepted contract\nCadder MUST satisfy the contract.\n\n#### Scenario: Contract holds\n- **WHEN** Cadder operates\n- **THEN** the contract holds\n"
      ),
    );
  }

  fn write_complete_main_specs(root: &Path) {
    write_main_specs_except(root, "");
  }

  fn write_main_specs_except(root: &Path, excluded: &str) {
    for (capability, prefix) in CAPABILITY_PREFIXES {
      if capability != excluded {
        write_main_spec(root, capability, &format!("{prefix}001"));
      }
    }
  }

  fn valid_delta_spec(id: &str) -> String {
    format!(
      "## ADDED Requirements\n\n### Requirement: {id}: New contract\nCadder MUST satisfy the contract.\n\n#### Scenario: Contract holds\n- **WHEN** Cadder operates\n- **THEN** the contract holds\n"
    )
  }

  fn valid_verification() -> &'static str {
    "## Evidence\n\n| Requirement ID | Task | Evidence | Result |\n| --- | --- | --- | --- |\n| `IPC-001` | `1.1` | `cargo test peer_auth` | Pass |\n\n## Gate\n\n- [x] Every task in `tasks.md` is complete.\n- [x] Every in-scope requirement ID has passing evidence.\n- [x] Focused tests pass.\n- [x] Repository checks required by the design pass.\n- [x] Documentation describes only verified behavior.\n- [x] `cargo xtask openspec-check` passes.\n"
  }

  fn write_file(root: &Path, relative: &str, contents: impl AsRef<[u8]>) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
  }
