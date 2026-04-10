# Messages Create - Normal (non-streaming) response
POST http://localhost:3000/v1/messages
Content-Type: application/json
{
  "model": "claude-haiku-4-5",
  "max_tokens": 64,
  "messages": [
    {
      "role": "user",
      "content": "Reply with exactly: Hello, World!"
    }
  ]
}

HTTP 200
[Asserts]
header "content-type" contains "application/json"
jsonpath "$.id" startsWith "msg_"
jsonpath "$.type" == "message"
jsonpath "$.role" == "assistant"
jsonpath "$.model" startsWith "claude-haiku"
jsonpath "$.content" count >= 1
jsonpath "$.content[0].type" == "text"
jsonpath "$.content[0].text" exists
jsonpath "$.stop_reason" exists
jsonpath "$.usage.input_tokens" >= 1
jsonpath "$.usage.output_tokens" >= 1


# Messages Create - Streaming response
POST http://localhost:3000/v1/messages
Content-Type: application/json
{
  "model": "claude-haiku-4-5",
  "max_tokens": 64,
  "stream": true,
  "messages": [
    {
      "role": "user",
      "content": "Reply with exactly: Hello, World!"
    }
  ]
}

HTTP 200
[Asserts]
header "content-type" contains "text/event-stream"
body contains "event: message_start"
body contains "event: content_block_start"
body contains "event: content_block_delta"
body contains "event: content_block_stop"
body contains "event: message_delta"
body contains "event: message_stop"


# Messages Create - With system prompt
POST http://localhost:3000/v1/messages
Content-Type: application/json
{
  "model": "claude-haiku-4-5",
  "max_tokens": 64,
  "system": "You are a helpful assistant that always responds in uppercase.",
  "messages": [
    {
      "role": "user",
      "content": "Say hi"
    }
  ]
}

HTTP 200
[Asserts]
jsonpath "$.type" == "message"
jsonpath "$.role" == "assistant"
jsonpath "$.content[0].type" == "text"
jsonpath "$.content[0].text" exists
jsonpath "$.stop_reason" exists


# Messages Create - Multi-turn conversation
POST http://localhost:3000/v1/messages
Content-Type: application/json
{
  "model": "claude-haiku-4-5",
  "max_tokens": 64,
  "messages": [
    {
      "role": "user",
      "content": "My name is Alice."
    },
    {
      "role": "assistant",
      "content": "Hello Alice!"
    },
    {
      "role": "user",
      "content": "What is my name?"
    }
  ]
}

HTTP 200
[Asserts]
jsonpath "$.type" == "message"
jsonpath "$.content[0].type" == "text"
jsonpath "$.content[0].text" contains "Alice"


# Messages Create - Streaming multi-turn conversation
POST http://localhost:3000/v1/messages
Content-Type: application/json
{
  "model": "claude-haiku-4-5",
  "max_tokens": 64,
  "stream": true,
  "messages": [
    {
      "role": "user",
      "content": "My name is Bob."
    },
    {
      "role": "assistant",
      "content": "Hello Bob!"
    },
    {
      "role": "user",
      "content": "What is my name?"
    }
  ]
}

HTTP 200
[Asserts]
header "content-type" contains "text/event-stream"
body contains "event: message_start"
body contains "event: message_stop"
body contains "Bob"


# Messages Create - Tool use (non-streaming)
POST http://localhost:3000/v1/messages
Content-Type: application/json
{
  "model": "claude-haiku-4-5",
  "max_tokens": 256,
  "messages": [
    {
      "role": "user",
      "content": "What is the weather in San Francisco? Use the get_weather tool."
    }
  ],
  "tools": [
    {
      "name": "get_weather",
      "description": "Get the current weather for a location.",
      "input_schema": {
        "type": "object",
        "properties": {
          "location": {
            "type": "string",
            "description": "City and state, e.g. San Francisco, CA"
          }
        },
        "required": ["location"]
      }
    }
  ],
  "tool_choice": {"type": "any"}
}

HTTP 200
[Asserts]
jsonpath "$.type" == "message"
jsonpath "$.stop_reason" == "tool_use"
jsonpath "$.content[0].type" == "tool_use"
jsonpath "$.content[0].name" == "get_weather"
jsonpath "$.content[0].name" not contains "mcp_"
jsonpath "$.content[0].id" exists
jsonpath "$.content[0].input.location" exists


# Messages Create - Tool use (streaming)
POST http://localhost:3000/v1/messages
Content-Type: application/json
{
  "model": "claude-haiku-4-5",
  "max_tokens": 256,
  "stream": true,
  "messages": [
    {
      "role": "user",
      "content": "What is the weather in San Francisco? Use the get_weather tool."
    }
  ],
  "tools": [
    {
      "name": "get_weather",
      "description": "Get the current weather for a location.",
      "input_schema": {
        "type": "object",
        "properties": {
          "location": {
            "type": "string",
            "description": "City and state, e.g. San Francisco, CA"
          }
        },
        "required": ["location"]
      }
    }
  ],
  "tool_choice": {"type": "any"}
}

HTTP 200
[Asserts]
header "content-type" contains "text/event-stream"
body contains "event: content_block_start"
body matches "\"type\":\\s*\"tool_use\""
body matches "\"name\":\\s*\"get_weather\""
body not contains "mcp_get_weather"
body contains "event: content_block_stop"
body contains "event: message_stop"


# Messages Create - Tool use with tool_choice name (non-streaming)
POST http://localhost:3000/v1/messages
Content-Type: application/json
{
  "model": "claude-haiku-4-5",
  "max_tokens": 256,
  "messages": [
    {
      "role": "user",
      "content": "What is the weather in San Francisco?"
    }
  ],
  "tools": [
    {
      "name": "get_weather",
      "description": "Get the current weather for a location.",
      "input_schema": {
        "type": "object",
        "properties": {
          "location": {
            "type": "string",
            "description": "City and state, e.g. San Francisco, CA"
          }
        },
        "required": ["location"]
      }
    }
  ],
  "tool_choice": {"type": "tool", "name": "get_weather"}
}

HTTP 200
[Asserts]
jsonpath "$.type" == "message"
jsonpath "$.stop_reason" == "tool_use"
jsonpath "$.content[0].type" == "tool_use"
jsonpath "$.content[0].name" == "get_weather"
jsonpath "$.content[0].name" not contains "mcp_"
jsonpath "$.content[0].id" exists
jsonpath "$.content[0].input.location" exists


# Messages Create - Tool use with tool_choice name (streaming)
POST http://localhost:3000/v1/messages
Content-Type: application/json
{
  "model": "claude-haiku-4-5",
  "max_tokens": 256,
  "stream": true,
  "messages": [
    {
      "role": "user",
      "content": "What is the weather in San Francisco?"
    }
  ],
  "tools": [
    {
      "name": "get_weather",
      "description": "Get the current weather for a location.",
      "input_schema": {
        "type": "object",
        "properties": {
          "location": {
            "type": "string",
            "description": "City and state, e.g. San Francisco, CA"
          }
        },
        "required": ["location"]
      }
    }
  ],
  "tool_choice": {"type": "tool", "name": "get_weather"}
}

HTTP 200
[Asserts]
header "content-type" contains "text/event-stream"
body contains "event: content_block_start"
body matches "\"type\":\\s*\"tool_use\""
body matches "\"name\":\\s*\"get_weather\""
body not contains "mcp_get_weather"
body contains "event: content_block_stop"
body contains "event: message_stop"
