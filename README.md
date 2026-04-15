# claude-auth-proxy

An HTTP proxy that authenticates Anthropic API calls with Claude Code's OAuth credentials.

## Overview

`claude-auth-proxy` is a small reverse proxy in front of `https://api.anthropic.com/v1/*`. It loads OAuth credentials from the same store used by the official `claude` CLI (the macOS Keychain or `~/.claude/.credentials.json`), refreshes them as needed, and rewrites every request so it passes Anthropic's OAuth-only billing validator: it strips `x-api-key`, attaches a `Bearer` token, injects the required Claude-Code identity prompt and billing/session headers, and obfuscates tool names so they survive upstream name validation. The result is that any standard Anthropic SDK can talk to Anthropic on behalf of a Claude Max / Pro / Claude Code subscription instead of an API key.

> **Unofficial.** Not affiliated with or endorsed by Anthropic. Use at your own risk and within the terms of your Claude subscription.

This project is a Rust implementation inspired by [`griffinmartin/opencode-claude-auth`](https://github.com/griffinmartin/opencode-claude-auth), which pioneered the request-rewriting approach used here.

## Prerequisites

- **Rust toolchain.** The workspace uses Rust **edition 2024** and `resolver = "3"` ([`Cargo.toml`](Cargo.toml)), so you need Rust **1.85+**. There is no `rust-toolchain.toml`; whatever `rustup default` resolves to is used.
- **An active Claude Max / Pro / Claude Code subscription** whose OAuth credentials live in either:
  - the macOS Keychain under the service name `Claude Code-credentials` (or a versioned variant `Claude Code-credentials-<hex>`), or
  - the file `~/.claude/.credentials.json` (overridable with `CLAUDE_CREDENTIALS_FILE`), or
  - the `CLAUDE_ACCESS_TOKEN` environment variable (a static bearer token).

  The simplest way to populate the first two is to install the official [`claude` CLI](https://docs.claude.com/claude-code) and sign in once.

Cross-platform: the proxy builds and runs on macOS, Linux, and Windows. macOS is the most-tested target and is the only platform where the Keychain credential source and the optional `install` / `uninstall` launchd helpers are available; on other platforms supply credentials via `CLAUDE_ACCESS_TOKEN` or a credentials file.

## Build

```sh
cargo build --release
```

The release binary is at `target/release/claude-auth-proxy`.

## Run

Start the server:

```sh
cargo run --release -- run
# or, after `cargo install --path .`
claude-auth-proxy run
```

By default the proxy listens on `0.0.0.0:3000`. Verify it is up and has credentials:

```sh
curl http://localhost:3000/health   # {"status":"ok"}
curl http://localhost:3000/ready    # {"status":"ready"} once a credential is loaded, else 503
```

Point a client at the proxy by setting its Anthropic base URL to the proxy's address and **omitting** any `x-api-key` header -- the proxy strips it and substitutes the OAuth bearer token. With the official Anthropic SDKs:

```sh
export ANTHROPIC_BASE_URL=http://localhost:3000
# no ANTHROPIC_API_KEY needed
```

### macOS background install (optional)

To run the proxy as a launchd user agent that starts at login and is kept alive:

```sh
claude-auth-proxy install      # writes ~/Library/LaunchAgents/com.claude-auth-proxy.plist
claude-auth-proxy uninstall    # unloads and removes the plist
```

Logs are written to `~/Library/Logs/claude-auth-proxy.log`. The installed plist persists only the listen address and timeout/retry-related settings as environment variables, plus `RUST_LOG=info`; it does **not** capture every CLI flag or transform-related environment variable. Re-run `install` after changing any persisted setting, and do not assume the launchd plist fully reproduces an arbitrary `run` invocation.

## Endpoints

| Method | Path           | Purpose                                                     |
| ------ | -------------- | ----------------------------------------------------------- |
| `GET`  | `/health`      | Liveness probe -- always `200 {"status":"ok"}`.             |
| `GET`  | `/ready`       | `200 {"status":"ready"}` once a credential is loaded, otherwise `503`. |
| `ANY`  | `/v1/{*rest}`  | Proxied to `https://api.anthropic.com/v1/{rest}`.           |

## Configuration

Every CLI flag has a matching environment variable. Resolution order is **CLI flag &gt; environment variable &gt; compiled default**.

| Variable | Default | Purpose |
| --- | --- | --- |
| `RUST_LOG` | `info` | `tracing-subscriber` filter (e.g. `claude_auth_proxy=debug`). |
| `CLAUDE_PROXY_HOST` | `0.0.0.0` | Listen address. |
| `CLAUDE_PROXY_PORT` | `3000` | Listen port. |
| `PROXY_CONNECT_TIMEOUT_SECS` | `10` | Upstream connect timeout, in seconds. |
| `PROXY_READ_TIMEOUT_SECS` | `600` | Upstream read timeout, in seconds. |
| `PROXY_MAX_RETRIES` | `3` | Total attempts for `429`, `529`, and transient network errors. |
| `PROXY_RETRY_ON_5XX` | `false` | Retry generic `5xx` responses (other than `529`). Accepts `true`/`false`/`1`/`0`/`yes`/`no`. |
| `PROXY_5XX_MAX_RETRIES` | `1` | Total attempts for generic `5xx` when `PROXY_RETRY_ON_5XX=true`. |
| `ANTHROPIC_CLI_VERSION` | `2.1.90` | Claude Code version reported in the billing header and default user-agent. |
| `CLAUDE_CODE_ENTRYPOINT` | `cli` | Entrypoint identifier sent in the billing header. |
| `ANTHROPIC_USER_AGENT` | derived from `cc_version` | Override the `User-Agent` header sent upstream. |
| `ANTHROPIC_BETA_FLAGS` | model defaults | Comma-separated list that **replaces** the built-in base beta flags. |
| `CLAUDE_PROXY_TOOL_NAME_HASH_LEN` | `8` | Minimum hex length for the obfuscated `t_<hash>` tool names. |
| `CLAUDE_PROXY_TOOL_NAME_MAX_HASH_LEN` | `16` | Maximum hex length the obfuscator will grow to on collisions. |
| `CLAUDE_ACCESS_TOKEN` | unset | Static OAuth bearer token. When set, bypasses the keychain/file providers entirely (useful for CI, Docker, and testing). |
| `CLAUDE_CREDENTIALS_FILE` | `~/.claude/.credentials.json` | Path to the OAuth credentials JSON file. |

Run `claude-auth-proxy run --help` for the full CLI form of every flag.

## How it works

- **Credential sources.** On startup the proxy picks `CLAUDE_ACCESS_TOKEN` if set; otherwise it loads OAuth credentials from the macOS Keychain (`Claude Code-credentials*` items) and/or the JSON credentials file, deduplicated by access token ([`claude-auth-providers/src/claude_code/mod.rs`](claude-auth-providers/src/claude_code/mod.rs)).
- **Token refresh.** When a token is within ~60 minutes of expiry, or when upstream returns `401`, the proxy POSTs to `https://claude.ai/v1/oauth/token` with the refresh token. If that fails it spawns the `claude` CLI to refresh, then re-reads the credential store and retries the request once with the new token ([`claude-auth-providers/src/claude_code/refresh.rs`](claude-auth-providers/src/claude_code/refresh.rs), [`src/main.rs`](src/main.rs)).
- **Request rewrite.** The proxy strips `x-api-key`, then attaches `Authorization: Bearer <token>`, `anthropic-version`, a merged `anthropic-beta`, `x-app: cli`, the resolved `User-Agent`, a per-request `x-client-request-id`, and a per-process `X-Claude-Code-Session-Id` ([`claude-auth-transform/src/lib.rs`](claude-auth-transform/src/lib.rs)). The body is rewritten to inject the billing header as a `system[]` entry, ensure the required identity prefix `You are Claude Code, Anthropic's official CLI for Claude.`, apply per-model beta and output-config overrides, and obfuscate every tool name to a `t_<hash>` form so it slips past upstream's tool-name validator ([`claude-auth-transform/src/transforms.rs`](claude-auth-transform/src/transforms.rs), [`claude-auth-transform/src/tool_names.rs`](claude-auth-transform/src/tool_names.rs)).
- **Response rewrite.** SSE streams are buffered to event boundaries and the obfuscated tool names are reverse-mapped back to their originals before the body reaches the client ([`claude-auth-transform/src/response.rs`](claude-auth-transform/src/response.rs)).
- **Retries.** Independent budgets are kept for `429`/`529`, generic `5xx`, and network/timeout errors. An integer `Retry-After` header is honored (capped at 60 s); otherwise linear backoff `(attempt + 1) * 2 s` is used ([`src/main.rs`](src/main.rs)).

## License

Licensed under the GNU Affero General Public License v3.0 or later (AGPL-3.0-or-later). See [LICENSE](LICENSE) for the full text.
