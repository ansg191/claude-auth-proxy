# Retrieve Model - By model ID
GET http://localhost:3000/v1/models/claude-haiku-4-5-20251001

HTTP 200
[Asserts]
header "content-type" contains "application/json"
jsonpath "$.id" == "claude-haiku-4-5-20251001"
jsonpath "$.type" == "model"
jsonpath "$.display_name" exists
jsonpath "$.created_at" exists
jsonpath "$.max_input_tokens" >= 1
jsonpath "$.max_tokens" >= 1


# Retrieve Model - By alias (resolves to full model ID)
GET http://localhost:3000/v1/models/claude-sonnet-4-6

HTTP 200
[Asserts]
jsonpath "$.type" == "model"
jsonpath "$.id" exists
jsonpath "$.display_name" exists


# Retrieve Model - Non-existent model returns error
GET http://localhost:3000/v1/models/claude-nonexistent-99

HTTP 404
[Asserts]
jsonpath "$.type" == "error"
jsonpath "$.error.type" == "not_found_error"
