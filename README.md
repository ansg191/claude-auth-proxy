# claude-auth-proxy

An HTTP proxy that authenticates Anthropic API calls with Claude Code's OAuth credentials.

Point any Anthropic-compatible client at the proxy and it just works -- the official [Python](https://github.com/anthropics/anthropic-sdk-python) and [TypeScript](https://github.com/anthropics/anthropic-sdk-typescript) SDKs, [OpenCode](https://github.com/sst/opencode), [HolmesGPT](https://github.com/HolmesGPT/holmesgpt), [litellm](https://github.com/BerriAI/litellm)-based agents, and anything else that speaks the Anthropic Messages API. Swap the base URL, use any placeholder for the API key, and your existing workflow routes through a Claude Max / Pro / Claude Code subscription with no code changes.

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

Point a client at the proxy by setting its Anthropic base URL to the proxy's address -- see [Pointing clients at the proxy](#pointing-clients-at-the-proxy) below for concrete examples.

### macOS background install (optional)

To run the proxy as a launchd user agent that starts at login and is kept alive:

```sh
claude-auth-proxy install      # writes ~/Library/LaunchAgents/com.claude-auth-proxy.plist
claude-auth-proxy uninstall    # unloads and removes the plist
```

Logs are written to `~/Library/Logs/claude-auth-proxy.log`. The installed plist persists only the listen address and timeout/retry-related settings as environment variables, plus `RUST_LOG=info`; it does **not** capture every CLI flag or transform-related environment variable. Re-run `install` after changing any persisted setting, and do not assume the launchd plist fully reproduces an arbitrary `run` invocation.

## Docker

A prebuilt multi-arch (`linux/amd64`, `linux/arm64`) image is published to GHCR at `ghcr.io/ansg191/claude-auth-proxy` by the [Docker workflow](.github/workflows/docker.yml). It bundles the official `claude` CLI alongside the proxy binary so OAuth onboarding can be completed from inside the container ([`Dockerfile`](Dockerfile)).

The image runs as the distroless `nonroot` user (UID `65532`), exposes port `3000`, and, when no `CLAUDE_ACCESS_TOKEN` is set, reads credentials from `/home/nonroot/.claude/.credentials.json` at startup.

### First run (OAuth onboarding)

Credentials are loaded once on startup, so mount a persistent volume at `/home/nonroot` before the first run -- otherwise the OAuth session has nowhere to survive a container restart.

```sh
docker volume create claude-auth-proxy-data

docker run -d \
  --name claude-auth-proxy \
  -p 3000:3000 \
  -v claude-auth-proxy-data:/home/nonroot \
  ghcr.io/ansg191/claude-auth-proxy:latest
```

The proxy starts immediately but `/ready` returns `503` because no credential is loaded yet. Launch the bundled `claude` CLI inside the running container to complete the OAuth flow:

```sh
docker exec -it claude-auth-proxy claude
```

Walk through the onboarding prompts. When asked how to authenticate:

1. Pick **"Sign in with Claude"** (the OAuth option, not `x-api-key`).
2. Choose the **manual / copy-paste code** flow -- the loopback-redirect option cannot work because the container is not the machine running your browser.
3. Open the printed URL on your host, authenticate, and paste the authorization code back into the CLI prompt.

The CLI writes the OAuth credentials to `/home/nonroot/.claude/.credentials.json` inside the persistent volume.

### Subsequent runs

The proxy only loads credentials at startup, so restart the container once after onboarding to pick them up:

```sh
docker restart claude-auth-proxy
curl http://localhost:3000/ready   # {"status":"ready"}
```

After that, the proxy refreshes the access token on its own (and re-spawns `claude` if an HTTP refresh ever fails). Any container with the same volume mounted at `/home/nonroot` sees the same OAuth identity, so you can destroy and recreate the container freely -- upgrades and config changes do not require redoing the flow.

For Kubernetes, the equivalent shape is a `StatefulSet` (or a `Deployment` with `replicas: 1`) plus a `PersistentVolumeClaim` mounted at `/home/nonroot`, wiring `readinessProbe` to `/ready` and `livenessProbe` to `/health` on port `3000`. Onboard once with `kubectl exec -it <pod> -- claude`, then `kubectl rollout restart` to reload.

### Headless / pre-existing bearer token

If you already have an OAuth bearer token and do not want to run the CLI at all, pass it via `CLAUDE_ACCESS_TOKEN` instead of mounting a volume:

```sh
docker run -d \
  --name claude-auth-proxy \
  -p 3000:3000 \
  -e CLAUDE_ACCESS_TOKEN=<your-token> \
  ghcr.io/ansg191/claude-auth-proxy:latest
```

With `CLAUDE_ACCESS_TOKEN` set, the proxy skips the keychain/file loaders entirely and will not attempt to refresh the token. Any other [configuration](#configuration) variable can be supplied the same way with `-e`.

## Pointing clients at the proxy

Any Anthropic-compatible client can use the proxy. Set two things:

- **Base URL** -- the proxy's address (e.g., `http://localhost:3000`). Whether to append `/v1` depends on the client; each example below calls it out.
- **API key** -- any non-empty placeholder (e.g., `placeholder`). The proxy strips the inbound `x-api-key` header and replaces any `Authorization` header with its own OAuth bearer, so the value is irrelevant ([`claude-auth-transform/src/lib.rs`](claude-auth-transform/src/lib.rs)).

### Anthropic Python / TypeScript SDK

The official [Python](https://github.com/anthropics/anthropic-sdk-python) and [TypeScript](https://github.com/anthropics/anthropic-sdk-typescript) SDKs respect `ANTHROPIC_BASE_URL` and append `/v1` themselves, so **omit** the suffix:

```sh
export ANTHROPIC_BASE_URL=http://localhost:3000
export ANTHROPIC_API_KEY=placeholder
```

Or inline:

```python
from anthropic import Anthropic
client = Anthropic(base_url="http://localhost:3000", api_key="placeholder")
```

```typescript
import Anthropic from "@anthropic-ai/sdk";
const client = new Anthropic({ baseURL: "http://localhost:3000", apiKey: "placeholder" });
```

### OpenCode

In `~/.config/opencode/opencode.json`, override the built-in `anthropic` provider's options. OpenCode uses `@ai-sdk/anthropic`, which expects the `/v1` suffix **included** in `baseURL`:

```json
{
  "$schema": "https://opencode.ai/config.json",
  "provider": {
    "anthropic": {
      "options": {
        "baseURL": "http://localhost:3000/v1",
        "apiKey": "placeholder"
      }
    }
  }
}
```

### Claude Code CLI

In `~/.claude/settings.json` (base URL **without** `/v1` -- the CLI appends it):

```json
{
  "env": {
    "ANTHROPIC_BASE_URL": "http://localhost:3000",
    "ANTHROPIC_API_KEY": "placeholder"
  }
}
```

Setting `ANTHROPIC_API_KEY` is what forces Claude Code to use header-based auth against the proxy instead of its own stored OAuth; the proxy then strips the key and attaches the bearer it manages.

### HolmesGPT

HolmesGPT routes through [litellm](https://github.com/BerriAI/litellm), which picks up `ANTHROPIC_BASE_URL` for any model id starting with `anthropic/`:

```sh
export ANTHROPIC_BASE_URL=http://localhost:3000
export ANTHROPIC_API_KEY=placeholder
holmes ask --model anthropic/claude-sonnet-4-5 "..."
```

Per-model entries in HolmesGPT's model-list YAML can alternatively set `api_base` and `api_key` directly.

### Anything else

If the client is built on the official Anthropic SDK or on litellm, the two env vars above are usually enough. Otherwise, look for an "Anthropic base URL" / "API endpoint" knob in its config and point it at the proxy -- the API key field can always be a placeholder.

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
