# Liveness probe
GET http://localhost:3000/health

HTTP 200
[Asserts]
header "content-type" contains "application/json"
jsonpath "$.status" == "ok"


# Readiness probe
GET http://localhost:3000/ready

HTTP *
[Asserts]
header "content-type" contains "application/json"
jsonpath "$.status" exists
