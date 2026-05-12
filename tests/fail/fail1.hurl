# Reproduce the original Claude usage-limit failure
# Caused by block-listed tool names
POST http://localhost:3000/v1/messages
Content-Type: application/json

{
  "model": "claude-opus-4-6",
  "max_tokens": 32000,
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
  "tools": [
    {
      "name": "bash",
      "description": "Executes a given bash command in a persistent shell session with optional timeout, ensuring proper handling and security measures.",
      "input_schema": {
        "type": "object",
        "properties": {
          "command": {
            "type": "string"
          }
        },
        "required": [
          "command"
        ]
      }
    },
    {
      "name": "read",
      "description": "Read a file or directory from the local filesystem.",
      "input_schema": {
        "type": "object",
        "properties": {
          "filePath": {
            "type": "string"
          }
        },
        "required": [
          "filePath"
        ]
      }
    },
    {
      "name": "glob",
      "description": "Fast file pattern matching tool that works with any codebase size",
      "input_schema": {
        "type": "object",
        "properties": {
          "pattern": {
            "type": "string"
          }
        },
        "required": [
          "pattern"
        ]
      }
    },
    {
      "name": "grep",
      "description": "Fast content search tool that works with any codebase size",
      "input_schema": {
        "type": "object",
        "properties": {
          "pattern": {
            "type": "string"
          }
        },
        "required": [
          "pattern"
        ]
      }
    },
    {
      "name": "edit",
      "description": "Performs exact string replacements in files.",
      "input_schema": {
        "type": "object",
        "properties": {
          "filePath": {
            "type": "string"
          },
          "oldString": {
            "type": "string"
          },
          "newString": {
            "type": "string"
          }
        },
        "required": [
          "filePath",
          "oldString",
          "newString"
        ]
      }
    },
    {
      "name": "write",
      "description": "Writes a file to the local filesystem.",
      "input_schema": {
        "type": "object",
        "properties": {
          "filePath": {
            "type": "string"
          },
          "content": {
            "type": "string"
          }
        },
        "required": [
          "filePath",
          "content"
        ]
      }
    },
    {
      "name": "task",
      "description": "Available agent types and the tools they have access to:",
      "input_schema": {
        "type": "object",
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
        ]
      }
    },
    {
      "name": "webfetch",
      "description": "Fetches content from a specified URL",
      "input_schema": {
        "type": "object",
        "properties": {
          "url": {
            "type": "string"
          },
          "format": {
            "type": "string"
          }
        },
        "required": [
          "url",
          "format"
        ]
      }
    },
    {
      "name": "todowrite",
      "description": "Use this tool to create and manage a structured task list for your current coding session.",
      "input_schema": {
        "type": "object",
        "properties": {
          "todos": {
            "type": "array"
          }
        },
        "required": [
          "todos"
        ]
      }
    },
    {
      "name": "skill",
      "description": "Load a specialized skill that provides domain-specific instructions and workflows.",
      "input_schema": {
        "type": "object",
        "properties": {
          "name": {
            "type": "string"
          }
        },
        "required": [
          "name"
        ]
      }
    },
    {
      "name": "question",
      "description": "Use this tool when you need to ask the user questions during execution.",
      "input_schema": {
        "type": "object",
        "properties": {
          "questions": {
            "type": "array"
          }
        },
        "required": [
          "questions"
        ]
      }
    }
  ],
  "tool_choice": {
    "type": "auto"
  },
  "stream": true
}

HTTP 200
[Asserts]
header "content-type" contains "text/event-stream"