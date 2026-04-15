# A non-thinking, streaming opus request
POST https://api.anthropic.com/v1/messages?beta=true
Accept: application/json
Authorization: Bearer {{token}}
Content-Type: application/json
User-Agent: claude-cli/2.1.92 (external, cli)
anthropic-beta: claude-code-20250219,oauth-2025-04-20,context-1m-2025-08-07,interleaved-thinking-2025-05-14,redact-thinking-2026-02-12,context-management-2025-06-27,prompt-caching-scope-2026-01-05,advanced-tool-use-2025-11-20,effort-2025-11-24
anthropic-version: 2023-06-01
x-app: cli

{
  "model": "claude-opus-4-6",
  "messages": [
    {
      "role": "user",
      "content": [
        {
          "type": "text",
          "text": "Reply with exactly: Hello, World!"
        }
      ]
    }
  ],
  "system": [
    {
      "type": "text",
      "text": "x-anthropic-billing-header: cc_version=2.1.92.085; cc_entrypoint=cli; cch=7b421;"
    },
    {
      "type": "text",
      "text": "You are Claude Code, Anthropic's official CLI for Claude."
    }
  ],
  "max_tokens": 64000,
  "stream": true
}


HTTP 200
[Asserts]
header "content-type" contains "text/event-stream"
body contains "data:"
body contains "event: message_start"
body contains "event: message_stop"
