# A nonthinking, streaming haiku request
POST https://api.anthropic.com/v1/messages
Accept: application/json
Authorization: Bearer {{token}}
Content-Type: application/json
User-Agent: claude-cli/2.1.92 (external, cli)
X-Claude-Code-Session-Id: 2f45817b-eb75-4af3-a2db-8eb7adba402e
anthropic-beta: oauth-2025-04-20,interleaved-thinking-2025-05-14,redact-thinking-2026-02-12,context-management-2025-06-27,prompt-caching-scope-2026-01-05,claude-code-20250219
anthropic-version: 2023-06-01
x-app: cli
x-client-request-id: 0898dd92-7070-4a42-9d7d-ffece9431513
Host: api.anthropic.com

{
  "model": "claude-haiku-4-5",
  "messages": [
    {
      "role": "user",
      "content": [
        {
          "type": "text",
          "text": "Reply with exactly: Hello, World!",
          "cache_control": {
            "type": "ephemeral",
            "ttl": "1h"
          }
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
      "text": "You are Claude Code, Anthropic's official CLI for Claude.",
      "cache_control": {
        "type": "ephemeral",
        "ttl": "1h"
      }
    }
  ],
  "max_tokens": 8000,
  "thinking": {
    "budget_tokens": 31999,
    "type": "enabled"
  },
  "stream": true
}


HTTP 200
[Asserts]
header "content-type" contains "text/event-stream"
body contains "data:"
body contains "event: message_start"
body contains "event: message_stop"
