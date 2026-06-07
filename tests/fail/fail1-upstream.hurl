
# Upstream request & response
POST https://api.anthropic.com/v1/messages
user-agent: claude-cli/2.1.90 (external, cli)
accept: */*
content-type: application/json
authorization: Bearer {{token}}
anthropic-version: 2023-06-01
anthropic-beta: claude-code-20250219,oauth-2025-04-20,interleaved-thinking-2025-05-14,prompt-caching-scope-2026-01-05,context-management-2025-06-27,effort-2025-11-24
x-app: cli
x-client-request-id: dd089411-aff3-4f79-aaff-c692a9a02a0b
x-claude-code-session-id: d12d9aee-62cb-4056-af3d-0d5a75b55ee2

{
  "model": "claude-opus-4-6",
  "system": [
    {
      "type": "text",
      "text": "x-anthropic-billing-header: cc_version=2.1.90.b39; cc_entrypoint=cli; cch=532ea;"
    },
    {
      "type": "text",
      "text": "You are Claude Code, Anthropic's official CLI for Claude."
    }
  ],
  "tools": [
    {
      "name": "mcp_bash",
      "description": "Executes a given bash command in a persistent shell session with optional timeout, ensuring proper handling and security measures.",
      "input_schema": {
        "properties": {
          "command": {
            "type": "string"
          }
        },
        "required": [
          "command"
        ],
        "type": "object"
      }
    },
    {
      "name": "mcp_read",
      "description": "Read a file or directory from the local filesystem.",
      "input_schema": {
        "properties": {
          "filePath": {
            "type": "string"
          }
        },
        "required": [
          "filePath"
        ],
        "type": "object"
      }
    },
    {
      "name": "mcp_glob",
      "description": "Fast file pattern matching tool that works with any codebase size",
      "input_schema": {
        "properties": {
          "pattern": {
            "type": "string"
          }
        },
        "required": [
          "pattern"
        ],
        "type": "object"
      }
    },
    {
      "name": "mcp_grep",
      "description": "Fast content search tool that works with any codebase size",
      "input_schema": {
        "properties": {
          "pattern": {
            "type": "string"
          }
        },
        "required": [
          "pattern"
        ],
        "type": "object"
      }
    },
    {
      "name": "mcp_edit",
      "description": "Performs exact string replacements in files.",
      "input_schema": {
        "properties": {
          "filePath": {
            "type": "string"
          },
          "newString": {
            "type": "string"
          },
          "oldString": {
            "type": "string"
          }
        },
        "required": [
          "filePath",
          "oldString",
          "newString"
        ],
        "type": "object"
      }
    },
    {
      "name": "mcp_write",
      "input_schema": {
        "properties": {
          "content": {
            "type": "string"
          },
          "filePath": {
            "type": "string"
          }
        },
        "required": [
          "filePath",
          "content"
        ],
        "type": "object"
      },
      "description": "Writes a file to the local filesystem."
    },
    {
      "name": "mcp_task",
      "description": "Available agent types and the tools they have access to:",
      "input_schema": {
        "properties": {
          "description": {
            "type": "string"
          },
          "prompt": {
            "type": "string"
          },
          "subagent_type": {
            "type": "string"
          }
        },
        "required": [
          "description",
          "prompt",
          "subagent_type"
        ],
        "type": "object"
      }
    },
    {
      "name": "mcp_webfetch",
      "description": "Fetches content from a specified URL",
      "input_schema": {
        "properties": {
          "format": {
            "type": "string"
          },
          "url": {
            "type": "string"
          }
        },
        "required": [
          "url",
          "format"
        ],
        "type": "object"
      }
    },
    {
      "name": "mcp_todowrite",
      "description": "Use this tool to create and manage a structured task list for your current coding session.",
      "input_schema": {
        "properties": {
          "todos": {
            "type": "array"
          }
        },
        "required": [
          "todos"
        ],
        "type": "object"
      }
    },
    {
      "name": "mcp_skill",
      "description": "Load a specialized skill that provides domain-specific instructions and workflows.",
      "input_schema": {
        "properties": {
          "name": {
            "type": "string"
          }
        },
        "required": [
          "name"
        ],
        "type": "object"
      }
    },
    {
      "name": "mcp_question",
      "description": "Use this tool when you need to ask the user questions during execution.",
      "input_schema": {
        "properties": {
          "questions": {
            "type": "array"
          }
        },
        "required": [
          "questions"
        ],
        "type": "object"
      }
    }
  ],
  "tool_choice": {
    "type": "auto"
  },
  "messages": [
    {
      "role": "user",
      "content": [
        {
          "type": "text",
          "text": "Test",
          "cache_control": {
            "type": "ephemeral"
          }
        }
      ]
    }
  ],
  "stream": true,
  "max_tokens": 32000
}


HTTP 400
[Asserts]
header "content-type" contains "application/json"