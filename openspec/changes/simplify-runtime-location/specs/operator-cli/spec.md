## MODIFIED Requirements

### Requirement: CLI-003: Global output options
The CLI SHALL accept the global options `--output <human|json|jsonl>` and `--no-color` before or after the subcommand path. Human output SHALL be the default. The CLI SHALL NOT expose runtime-directory or profile-selection options. Every command SHALL use the runtime root derived from the parent directory of its running executable, independent of the current working directory. A command MUST reject an output mode that cannot represent its result before connecting or changing state.

#### Scenario: Runtime root follows the executable
- **WHEN** an operator invokes `cadder tui` from a working directory different from the directory containing `cadder`
- **THEN** the operator attaches only to the runtime rooted beside the `cadder` executable

#### Scenario: Runtime selection options are rejected
- **WHEN** an operator supplies a runtime-directory or profile-selection option
- **THEN** the CLI returns the invalid-input exit code without connecting to the daemon or changing files

#### Scenario: Tail requests buffered JSON
- **WHEN** an operator selects `--output json` for `logs tail`
- **THEN** the CLI exits with code `2` before connecting
- **AND** it explains that an unbounded tail supports only human or JSONL output
