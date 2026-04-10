# List Models - Default
GET http://localhost:3000/v1/models

HTTP 200
[Asserts]
header "content-type" contains "application/json"
jsonpath "$.data" count >= 1
jsonpath "$.data[0].id" exists
jsonpath "$.data[0].type" == "model"
jsonpath "$.data[0].display_name" exists
jsonpath "$.data[0].created_at" exists
jsonpath "$.has_more" exists
jsonpath "$.first_id" exists
jsonpath "$.last_id" exists


# List Models - With limit
GET http://localhost:3000/v1/models?limit=2

HTTP 200
[Asserts]
jsonpath "$.data" count <= 2
jsonpath "$.data[0].type" == "model"
jsonpath "$.data[0].id" exists


# List Models - Pagination with after_id
# First, get the first page to obtain a cursor
GET http://localhost:3000/v1/models?limit=1

HTTP 200
[Captures]
last_model_id: jsonpath "$.last_id"
[Asserts]
jsonpath "$.data" count == 1

# Then fetch the next page
GET http://localhost:3000/v1/models?limit=1&after_id={{last_model_id}}

HTTP 200
[Asserts]
jsonpath "$.data" count >= 1
jsonpath "$.data[0].type" == "model"
