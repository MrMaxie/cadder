## ADDED Requirements

### Requirement: DOC-004: Installation guidance distinguishes npm, Cadder, and Caddy

Public installation guidance SHALL present `npx cadder`, global npm installation, and portable GitHub archives only when the corresponding channel has passed its release verification. It MUST explain that npm installs the Cadder operator, daemon, and `caddy` shim while the upstream Caddy web server remains a separate prerequisite.

The hero and getting-started path MUST give users adjacent actions for npm installation or download and documentation without presenting an unpublished package as available.

#### Scenario: npm channel is not yet public

- **WHEN** the current Cadder version is not verified in the public npm registry
- **THEN** published documentation does not instruct users to install that version from npm

#### Scenario: npm channel is verified

- **WHEN** the exact Cadder version passes clean-room registry installation on every supported platform
- **THEN** the hero and installation guide may present npm as a supported option
- **AND** they retain a direct GitHub download path and the separate upstream Caddy requirement
