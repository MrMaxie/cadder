## 1. Generated release workflow

- [x] 1.1 Update cargo-dist to 0.33.0 and regenerate its official GitHub workflow. (verification: cargo-dist planning succeeds on the updated pinned tool)
- [x] 1.2 Add and test the deterministic workflow postprocessor for least-privilege permissions and validated tag data. (verification: apply and check modes accept the committed workflow and reject each missing invariant)
- [x] 1.3 Route the supported mise release and complete-check tasks through generated-then-hardened workflow verification. (verification: local and CI task definitions use the same focused check)

## 2. Exact npm release provenance

- [x] 2.1 Check out the selected release tag and resolve its exact commit before downloading assets. (verification: workflow structure tests require the exact ref and exported commit)
- [x] 2.2 Bind each artifact attestation to the selected ref, commit digest, repository, and release workflow. (verification: assembly tests require every provenance argument and reject missing or mismatched identity)
- [x] 2.3 Refresh the documentation lockfile so `devalue` resolves to at least 5.9.4 without upgrading direct documentation dependencies. (verification: npm audit passes and the vulnerable lock entry is absent)

## 3. Complete validation

- [x] 3.1 Run workflow, npm, cargo-dist, OpenSpec, audit, and complete repository gates, then review the full diff and working tree. (verification: all applicable gates pass with no publication or unrelated files)
