---
"@alienplatform/platform-api": major
"@alienplatform/manager-api": major
---

Simplify TypeScript SDK calls for endpoints with request bodies. Methods
with one path parameter now accept `(id, body)` instead of a nested request
object. Body-only methods accept the body directly, while methods with more
parameters retain named request objects. Query-only method signatures and
HTTP request formats are unchanged.

Use `deploymentGroups.createToken(id, body)` in place of
`deploymentGroups.createDeploymentGroupToken({ id, createDeploymentGroupTokenRequest: body })`.
