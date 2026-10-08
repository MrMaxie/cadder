## 1. Configuration and trusted resolution

- [x] 1.1 `[REG-004, CADDY-001]` Implement strict existing TOML selectors and released configuration locations/precedence (verification: focused config tests include unknown fields, conflicting selectors and malformed higher-priority denial).
- [x] 1.2 `[CADDY-001, SHIM-003]` Resolve trusted native Caddy, deny recursive npm/SEA/alias identities and pin immutable image/compatibility evidence (verification: source/layout, wrapper, hardlink, changed-image and version/module tests).
- [x] 1.3 `[CADDY-005]` Bound owned probe/process streams, deadlines and teardown without unrelated process termination (verification: real fake-child stream/timeout/abort/descendant tests; native gaps remain explicit).

## 2. Adaptation and validation

- [x] 2.1 `[REG-004, CADDY-005]` Adapt existing config paths and adapter metadata through the pinned executable into a complete typed JSON candidate (verification: real argv, canonical/raw paths, retained defaults, malformed output and nonzero-exit cases).
- [x] 2.2 `[CADDY-001, CADDY-005]` Validate a bounded JSON candidate with the existing validation-port signature and remove owned staging on every outcome (verification: success, rejection, timeout/overflow/spawn/pin failures and no leftover temp files).

## 3. Evidence and closeout

- [x] 3.1 `[REG-004, CADDY-001, CADDY-005, SHIM-003]` Integrate independent MiMo findings, full relevant validation and requirement-to-evidence reconciliation (verification: nub run check >=85% own lines; main/roadmap strict checks; diff inspection; record native/residual blockers without closing G1/G2/G3).

Implementation and the stated slice checks are complete; evidence is in
verification.md. RUN-003 server shutdown remains roadmap 3.8, not a delivered
requirement here. Native Unix/real-Caddy/channel proofs and the OpenSpec 1.5
custom-schema archive blocker remain explicit. Checked tasks do not close
CADDY-005 as a whole, G3, release acceptance or the archive gate.
