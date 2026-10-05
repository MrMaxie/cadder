## ADDED Requirements

### Requirement: DOC-001: English target-state documentation
All Cadder project documentation SHALL be written in English, and public user documentation SHALL describe only the accepted Cadder 1.0 product in clear present-tense language.

Public pages MUST NOT use implementation progress, planned work, or workstation state to qualify accepted behavior. Delivery status belongs to OpenSpec changes and verification evidence rather than the user documentation.

#### Scenario: Accepted behavior documentation
- **WHEN** an accepted Cadder 1.0 behavior is described on a public page
- **THEN** the page explains the behavior in present tense without implementation-status language

#### Scenario: Delivery status review
- **WHEN** a contributor needs to determine whether an accepted requirement is implemented
- **THEN** contributor documentation directs them to the relevant OpenSpec change and verification evidence
- **AND** the public page remains focused on product use

### Requirement: DOC-002: Audience-directed information architecture
The documentation site SHALL organize content by user intent using distinct Getting Started, How-to Guides, Troubleshooting, Reference, and Explanation sections.

Content for end users, operators, automation authors, contributors, and release maintainers MUST appear in the section and level of detail appropriate to that audience. Contributor and release-maintainer procedures MUST NOT interrupt task-oriented user guidance.

#### Scenario: New user onboarding
- **WHEN** a new user opens the documentation
- **THEN** Getting Started provides the shortest supported path from installation through a managed project and successful CLI or TUI inspection

#### Scenario: Operator recovery
- **WHEN** an operator encounters a known daemon, shim, Caddy, permission, or IIS failure
- **THEN** Troubleshooting provides symptom-based diagnosis and safe recovery steps without requiring repository knowledge

#### Scenario: Contributor guidance
- **WHEN** a contributor needs build, test, architecture, or release-maintenance instructions
- **THEN** those instructions are available in contributor documentation rather than embedded in the end-user workflow

### Requirement: DOC-003: Documentation environment boundary
End-user Cadder documentation MUST remain independent of any maintainer's workstation, private operational workspace, repository checkout, or development-only backend.

End-user pages SHALL use portable placeholders and product-level concepts. They MUST NOT expose `.local`, personal absolute paths, checkout-specific variables, mock-only commands, agent instructions, or repository task-runner details. Contributor and release-maintainer documentation MAY describe checked-in build commands, `cargo xtask` workflows, and supported development fixtures, but MUST remain free of personal workstation paths and private operational data.

#### Scenario: End-user content boundary scan
- **WHEN** end-user documentation is validated
- **THEN** it contains no `.local` references, personal filesystem paths, checkout-specific variables, mock-only workflows, or agent-only instructions

#### Scenario: Portable path example
- **WHEN** a command requires a path example
- **THEN** the documentation uses a platform-appropriate placeholder or standard installation location instead of a maintainer-specific path

#### Scenario: Contributor runs repository validation
- **WHEN** contributor documentation explains the supported repository gate
- **THEN** it may name checked-in Cargo and xtask commands
- **AND** it does not require `.local`, a private backend, or a maintainer-specific path

### Requirement: DOC-004: Complete user journeys
The public documentation SHALL cover installation, real-Caddy prerequisites, explicit shim alias setup and removal, first project registration, CLI and TUI operation, upgrades, uninstall, diagnostics, recovery, and Windows IIS handoff where supported.

Each journey MUST state prerequisites, the expected successful result, relevant safety constraints, and a link to troubleshooting or reference material when the operation can fail.

#### Scenario: First managed project
- **WHEN** a user follows the documented first-project journey on a supported platform
- **THEN** the user installs Cadder, configures a trusted real Caddy source, explicitly sets up the shim alias, starts the daemon, runs a project, and verifies its state through `cadder`

#### Scenario: Safe uninstall guidance
- **WHEN** a user follows the uninstall journey
- **THEN** the documentation explains which Cadder-owned resources are removed, which user data is preserved by default, and how the independent real Caddy installation is protected

### Requirement: DOC-005: Executable examples and generated reference
Every published Cadder CLI example SHALL be executable against the release candidate it documents, and generated command or machine-contract reference MUST come from the same source revision as the binaries.

An invalid command, unexpected exit code, stale output envelope, or generated-reference diff MUST fail documentation validation before publication.

#### Scenario: Executable command example
- **WHEN** documentation validation runs against a release candidate
- **THEN** each marked CLI example completes with its documented exit status and output contract

#### Scenario: Stale generated reference
- **WHEN** generated CLI or machine-contract reference differs from the checked-in documentation
- **THEN** validation fails and reports the source and generated document that disagree

### Requirement: DOC-006: Accessible documentation experience
The documentation site SHALL conform to WCAG 2.2 Level AA for the supported documentation journeys and MUST remain operable with a keyboard and understandable with a screen reader.

Navigation, headings, links, code examples, controls, status messages, focus indicators, color contrast, and responsive layouts MUST retain their meaning without relying only on color, pointer input, or a wide viewport.

#### Scenario: Automated accessibility validation
- **WHEN** the built documentation site is scanned at desktop and narrow viewport sizes
- **THEN** no Level A or AA accessibility violation is present in the primary navigation and documented user journeys

#### Scenario: Keyboard-only navigation
- **WHEN** a user navigates the site without a pointing device
- **THEN** every interactive control is reachable in a logical order, has a visible focus state, and can be activated from the keyboard

#### Scenario: Screen-reader review
- **WHEN** a release candidate undergoes manual screen-reader verification
- **THEN** page structure, navigation landmarks, headings, code examples, and status content are announced with an understandable name and order

### Requirement: DOC-007: Documentation validation and deployment gate
Documentation SHALL build and produce a reviewable preview for proposed changes, but public deployment MUST occur only from the exact source revision that passes the verified 1.0 publication gate defined by `DST-008`.

A preview, branch build, manual documentation run, or successful site build alone MUST NOT authorize public deployment.

#### Scenario: Pull request preview
- **WHEN** documentation or a documented public contract changes before release
- **THEN** validation builds the site and makes a non-public review artifact available
- **AND** no public Pages deployment occurs

#### Scenario: Unverified default-branch build
- **WHEN** documentation builds successfully on the default branch without a completed 1.0 release gate
- **THEN** the build remains unpublished

#### Scenario: Verified documentation deployment
- **WHEN** the complete 1.0 gate passes and release assets are published from one source revision
- **THEN** the documentation built from that same revision can be deployed publicly

### Requirement: DOC-008: Release and link consistency
Published documentation SHALL contain valid internal navigation and external references, and every version-specific download link, artifact name, checksum instruction, platform statement, and license statement MUST match the verified release it describes.

#### Scenario: Documentation release consistency
- **WHEN** a documentation candidate is checked against the release manifest
- **THEN** its artifact names, supported platforms, version, checksums, installation forms, and Apache-2.0 statement match the release manifest

#### Scenario: Broken reference
- **WHEN** an internal link, required external reference, or release download target cannot be resolved
- **THEN** documentation validation fails before public deployment and identifies the source page
