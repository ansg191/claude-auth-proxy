# A medium-effort, adaptive-thinking sonnet request
POST https://api.anthropic.com/v1/messages?beta=true
Accept: application/json
Authorization: Bearer {{token}}
Content-Type: application/json
User-Agent: claude-cli/2.1.92 (external, cli)
anthropic-beta: claude-code-20250219,oauth-2025-04-20,interleaved-thinking-2025-05-14,redact-thinking-2026-02-12,context-management-2025-06-27,prompt-caching-scope-2026-01-05,advanced-tool-use-2025-11-20,effort-2025-11-24
anthropic-version: 2023-06-01
x-app: cli

{
  "model": "claude-sonnet-4-6",
  "messages": [
    {
      "role": "user",
      "content": [
        {
          "type": "text",
          "text": "Compute 997 * 991. Think before answering, then reply with only the number."
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
  "max_tokens": 32000,
  "thinking": {
    "type": "adaptive"
  },
  "output_config": {
    "effort": "medium"
  },
  "stream": false
}


HTTP 200
[Asserts]
header "content-type" contains "application/json"
jsonpath "$.id" startsWith "msg_"
jsonpath "$.type" == "message"
jsonpath "$.role" == "assistant"
jsonpath "$.model" startsWith "claude-sonnet"
jsonpath "$.content" count >= 1
jsonpath "$.content[0].type" == "thinking"
jsonpath "$.content[0].thinking" exists
jsonpath "$.content[1].type" == "text"
jsonpath "$.content[1].text" exists
jsonpath "$.stop_reason" exists
jsonpath "$.usage.input_tokens" >= 1
jsonpath "$.usage.output_tokens" >= 1
