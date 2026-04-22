# /// script
# requires-python = ">=3.10"
# dependencies = [
#     "anthropic>=0.40",
# ]
# ///
"""
Smoke test for claude-auth-proxy against the official Anthropic Python SDK.

Verifies the README claim that pointing the SDK at the proxy "just works":
- ANTHROPIC_BASE_URL = proxy URL (no /v1 suffix, SDK appends it)
- ANTHROPIC_API_KEY  = any placeholder (proxy strips x-api-key)
"""

from __future__ import annotations

import os
import sys

import anthropic


BASE_URL = os.environ.get("ANTHROPIC_BASE_URL", "http://localhost:3000")
PLACEHOLDER_API_KEY = os.environ.get("ANTHROPIC_API_KEY", "sk-ant-placeholder-NOT-A-REAL-KEY")
MODEL = os.environ.get("CLAUDE_MODEL", "claude-haiku-4-5")
PROMPT = "Reply with exactly the string: PROXY_OK"


def main() -> int:
    print(f"SDK version : anthropic=={anthropic.__version__}")
    print(f"Base URL    : {BASE_URL}")
    print(f"API key     : {PLACEHOLDER_API_KEY[:12]}... (placeholder)")
    print(f"Model       : {MODEL}")
    print()

    client = anthropic.Anthropic(base_url=BASE_URL, api_key=PLACEHOLDER_API_KEY)

    try:
        resp = client.messages.create(
            model=MODEL,
            max_tokens=32,
            messages=[{"role": "user", "content": PROMPT}],
        )
    except anthropic.APIError as e:
        print(f"FAIL: Anthropic API error -> {type(e).__name__}: {e}")
        return 1
    except Exception as e:
        print(f"FAIL: unexpected error -> {type(e).__name__}: {e}")
        return 2

    text_parts = [b.text for b in resp.content if getattr(b, "type", None) == "text"]
    text = "".join(text_parts).strip()

    print(f"Response id     : {resp.id}")
    print(f"Response model  : {resp.model}")
    print(f"Stop reason     : {resp.stop_reason}")
    print(f"Input tokens    : {resp.usage.input_tokens}")
    print(f"Output tokens   : {resp.usage.output_tokens}")
    print(f"Response text   : {text!r}")

    if "PROXY_OK" in text:
        print("\nPASS: proxy round-trip succeeded with placeholder API key.")
        return 0
    else:
        print(
            "\nPARTIAL: request succeeded but response did not contain expected marker. "
            "Proxy is working (auth + routing OK), model just returned different text."
        )
        return 0


if __name__ == "__main__":
    sys.exit(main())
