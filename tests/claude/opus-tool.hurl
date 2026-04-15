# A simple non-thinking, non-streaming opus request w/ tools
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
          "text": "Run `echo Hello, World!` with the bash tool."
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
  "tools": [
    {
      "name": "bash",
      "description": "Executes a given bash command and returns its output.",
        "input_schema": {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "properties": {
          "command": {
            "description": "The command to execute",
            "type": "string"
          },
          "timeout": {
            "description": "Optional timeout in milliseconds (max 600000)",
            "type": "number"
          },
          "description": {
            "description": "Clear, concise description of what this command does in active voice",
            "type": "string"
          },
          "run_in_background": {
            "description": "Set to true to run this command in the background. Use Read to read the output later.",
            "type": "boolean"
          },
          "dangerouslyDisableSandbox": {
            "description": "Set this to true to dangerously override sandbox mode and run commands without sandboxing.",
            "type": "boolean"
          }
        },
        "required": [
          "command"
        ],
        "additionalProperties": false
      }
    }
  ],
  "tool_choice": {
    "type": "any"
  },
  "stream": false
}


HTTP 200
[Asserts]
header "content-type" contains "application/json"
jsonpath "$.id" startsWith "msg_"
jsonpath "$.type" == "message"
jsonpath "$.role" == "assistant"
jsonpath "$.model" startsWith "claude-opus"
jsonpath "$.content" count >= 1
jsonpath "$.content[0].type" == "tool_use"
jsonpath "$.content[0].id" exists
jsonpath "$.content[0].name" == "bash"
jsonpath "$.stop_reason" exists
jsonpath "$.usage.input_tokens" >= 1
jsonpath "$.usage.output_tokens" >= 1
