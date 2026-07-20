  #[test]
  fn coverage_command_args_emit_workspace_lcov_report() {
    let args = coverage_command_args(Path::new("target/custom/summary.lcov")).unwrap();

    let expected_tail = [
      "llvm-cov".to_string(),
      "--workspace".to_string(),
      "--lcov".to_string(),
      "--output-path".to_string(),
      "target/custom/summary.lcov".to_string(),
    ];
    let tail_start = args.len() - expected_tail.len();

    assert_eq!(&args[tail_start..], expected_tail);
    assert!(tail_start <= 1, "{args:?}");
    if tail_start == 1 {
      assert!(args[0].starts_with('+'), "{args:?}");
    }
  }

  #[test]
  fn read_lcov_line_coverage_sums_file_records() {
    let dir = unique_temp_dir("lcov-line-coverage");
    fs::create_dir_all(&dir).unwrap();
    let report = dir.join("coverage.lcov");
    fs::write(
      &report,
      "TN:\nSF:first.rs\nDA:10,1\nDA:11,0\nDA:11,3\nend_of_record\nTN:\nSF:second.rs\nDA:20,5\nDA:21,0\nend_of_record\n",
    )
    .unwrap();

    let coverage = read_lcov_line_coverage(&report).unwrap();

    assert_eq!(
      coverage,
      LcovLineCoverage {
        covered: 3,
        total: 4
      }
    );
    assert_eq!(coverage.percent(), 75.0);
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn read_lcov_line_coverage_accepts_da_checksum_fields() {
    let dir = unique_temp_dir("lcov-line-coverage-checksum");
    fs::create_dir_all(&dir).unwrap();
    let report = dir.join("coverage.lcov");
    fs::write(
      &report,
      "TN:\nSF:lib.rs\nDA:10,1,0123456789abcdef\nDA:11,0,abcdef0123456789\nend_of_record\n",
    )
    .unwrap();

    let coverage = read_lcov_line_coverage(&report).unwrap();

    assert_eq!(
      coverage,
      LcovLineCoverage {
        covered: 1,
        total: 2
      }
    );
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn read_lcov_line_coverage_rejects_empty_reports() {
    let dir = unique_temp_dir("empty-lcov-line-coverage");
    fs::create_dir_all(&dir).unwrap();
    let report = dir.join("coverage.lcov");
    fs::write(&report, "TN:\nend_of_record\n").unwrap();

    let error = read_lcov_line_coverage(&report).unwrap_err();

    assert!(error.to_string().contains("did not contain any DA"));
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn read_lcov_line_coverage_rejects_da_before_source_file() {
    let dir = unique_temp_dir("lcov-da-before-source-file");
    fs::create_dir_all(&dir).unwrap();
    let report = dir.join("coverage.lcov");
    fs::write(&report, "TN:\nDA:1,1\nend_of_record\n").unwrap();

    let error = read_lcov_line_coverage(&report).unwrap_err();

    assert!(error.to_string().contains("DA record appeared before SF"));
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn read_lcov_line_coverage_rejects_malformed_da_records() {
    let dir = unique_temp_dir("malformed-lcov-da");
    fs::create_dir_all(&dir).unwrap();
    let report = dir.join("coverage.lcov");
    fs::write(&report, "TN:\nSF:lib.rs\nDA:abc,1\nend_of_record\n").unwrap();

    let error = read_lcov_line_coverage(&report).unwrap_err();

    assert!(error.to_string().contains("parse DA source line"));
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn enforce_lcov_line_threshold_accepts_exact_threshold() {
    let dir = unique_temp_dir("threshold-lcov-line-coverage");
    fs::create_dir_all(&dir).unwrap();
    let report = dir.join("coverage.lcov");
    fs::write(&report, lcov_report_with_line_counts(85, 100)).unwrap();

    enforce_lcov_line_threshold(&report, 85.0).unwrap();

    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn enforce_lcov_line_threshold_reports_low_coverage() {
    let dir = unique_temp_dir("low-lcov-line-coverage");
    fs::create_dir_all(&dir).unwrap();
    let report = dir.join("coverage.lcov");
    fs::write(&report, lcov_report_with_line_counts(84, 100)).unwrap();

    let error = enforce_lcov_line_threshold(&report, 85.0).unwrap_err();

    assert!(error.to_string().contains("below required 85.00%"));
    assert!(error.to_string().contains("84/100"));
    fs::remove_dir_all(&dir).unwrap();
  }

  fn lcov_report_with_line_counts(covered: u64, total: u64) -> String {
    let mut report = "TN:\nSF:lib.rs\n".to_string();
    for line in 1..=total {
      let count = u64::from(line <= covered);
      report.push_str(&format!("DA:{line},{count}\n"));
    }
    report.push_str("end_of_record\n");
    report
  }

  #[test]
  fn coverage_command_args_use_workspace_lcov_without_package_exclusions() {
    assert!(COVERAGE_EXCLUDED_PACKAGES.is_empty());
    assert!(COVERAGE_IGNORED_FILENAME_REGEX.is_empty());
  }

  #[test]
  fn ensure_parent_dir_creates_report_directory() {
    let dir = unique_temp_dir("coverage-report-parent");
    let report_path = dir.join("nested").join("summary.lcov");

    ensure_parent_dir(&report_path).unwrap();

    assert!(dir.join("nested").is_dir());
    fs::remove_dir_all(&dir).unwrap();
  }
