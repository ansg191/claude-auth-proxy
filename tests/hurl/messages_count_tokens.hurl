# Count Tokens - Simple message
POST http://localhost:3000/v1/messages/count_tokens
Content-Type: application/json
anthropic-beta: token-counting-2024-11-01
{
  "model": "claude-haiku-4-5",
  "messages": [
    {
      "role": "user",
      "content": "Hello, Claude"
    }
  ]
}

HTTP 200
[Asserts]
header "content-type" contains "application/json"
jsonpath "$.input_tokens" >= 1


# Count Tokens - With system prompt
POST http://localhost:3000/v1/messages/count_tokens
Content-Type: application/json
anthropic-beta: token-counting-2024-11-01
{
  "model": "claude-haiku-4-5",
  "system": "You are a helpful assistant.",
  "messages": [
    {
      "role": "user",
      "content": "Hello, Claude"
    }
  ]
}

HTTP 200
[Asserts]
jsonpath "$.input_tokens" >= 1


# Count Tokens - Multi-turn conversation
POST http://localhost:3000/v1/messages/count_tokens
Content-Type: application/json
anthropic-beta: token-counting-2024-11-01
{
  "model": "claude-haiku-4-5",
  "messages": [
    {
      "role": "user",
      "content": "Hello there."
    },
    {
      "role": "assistant",
      "content": "Hi, I'm Claude. How can I help you?"
    },
    {
      "role": "user",
      "content": "Can you explain LLMs in plain English?"
    }
  ]
}

HTTP 200
[Asserts]
jsonpath "$.input_tokens" >= 1


# Count Tokens - With tools
POST http://localhost:3000/v1/messages/count_tokens
Content-Type: application/json
anthropic-beta: token-counting-2024-11-01
{
  "model": "claude-haiku-4-5",
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
  ]
}

HTTP 200
[Asserts]
jsonpath "$.input_tokens" >= 1
