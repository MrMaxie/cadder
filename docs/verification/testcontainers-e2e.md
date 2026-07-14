# Testcontainers E2E Verification

Use this checklist when running the Docker-required Cadder end-to-end suite locally or when
investigating the CI Docker E2E job.

## Purpose

The suite validates compiled host Cadder binaries against real Caddy in a disposable container. It
complements the fast fake-Caddy lifecycle tests and does not replace `cargo test --workspace`.

## Prerequisites

- Docker daemon is running.
- Docker CLI is available on `PATH`.
- The current user can start containers and pull `caddy:2.10.0-alpine`.
- No host-global Caddy installation is required.

## Command

Run from the repository root:

```sh
cargo build -p cadder-daemon -p cadder-shim
cargo test -p cadder-daemon --features docker-e2e --test testcontainers_e2e -- --ignored --test-threads=1
```

The test target is ignored and gated behind the `docker-e2e` feature, so it is skipped by default.
If Docker is unavailable, the explicit command should fail with a Docker/Testcontainers startup
error.

## Expected Coverage

- Host `cadderd` and host `caddy` shim binaries are copied into one temporary portable directory.
- Testcontainers starts an official Caddy container with dynamic host port mapping.
- A wrapper command delegates `adapt`, `run`, `reload`, and `stop` to real Caddy in the container.
- Two shim sessions register with one daemon and serve distinct HTTP responses through the mapped
  container port.
- Domain disable/enable reloads real Caddy and removes/restores the served route.
- Shim unregister/exit cleanup removes only the exiting registration.
- Duplicate domains report `domain-conflict`.
- Invalid Caddyfiles report `adapt-failed`.
- Daemon shutdown stops real Caddy and terminates `cadderd` while the container is still alive.

## Run Record

- Date:
- Platform:
- Docker version:
- Caddy image:
- Command:
- Result:
- Notes:
