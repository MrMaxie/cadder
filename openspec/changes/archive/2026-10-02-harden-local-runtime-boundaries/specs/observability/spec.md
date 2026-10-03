## MODIFIED Requirements

### Requirement: OBS-001: Redaction precedes persistence and response
Cadder MUST redact authorization and cookie header values, token and password values in supported text, JSON, and URL query forms, and private-key blocks before a log reaches SQLite or an IPC response. Retained data SHALL use one stable redaction marker and MUST NOT preserve any part of the matched credential value.

#### Scenario: Documented credential form appears
- **WHEN** a log contains an authorization header, cookie, token, password, or private key
- **THEN** the retained value contains the stable redaction marker instead of the credential value

## ADDED Requirements

### Requirement: OBS-004: Terminal rendering is control-safe
The operator CLI MUST neutralize ANSI escape sequences and C0 or C1 control characters in diagnostic and log messages before writing them to an interactive terminal. Redacted retained log content SHALL otherwise remain unchanged.

#### Scenario: Diagnostic contains terminal controls
- **WHEN** a retained log or diagnostic message contains an ANSI sequence or another C0 or C1 control character
- **THEN** the CLI renders the message without executing or forwarding that control sequence
