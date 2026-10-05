## MODIFIED Requirements

### Requirement: QT-008: New npm names use an explicit bootstrap

Before the first trusted release, each new npm package name SHALL be created interactively with account 2FA using a non-executable bootstrap version and an explicit non-default distribution tag. If the registry also assigns its required default tag to the package's only published version, that tag MAY temporarily point to the same non-executable bootstrap. Trusted publisher configuration and token restrictions MUST be applied only after the package names exist and before a real Cadder version is staged. The first verified release MUST replace the temporary default tag before npm installation is documented publicly.

#### Scenario: Package namespace is initialized

- **WHEN** maintainers bootstrap the npm package set
- **THEN** the default tag exposes no executable Cadder application and may point only to the non-executable bootstrap until the first verified release
- **AND** each package can be bound to the exact stage-only GitHub trusted publisher
