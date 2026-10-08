# VerifyContainerRegistryResponse

Verification requested

## Example Usage

```typescript
import { VerifyContainerRegistryResponse } from "@alienplatform/platform-api/models/operations";

let value: VerifyContainerRegistryResponse = {
  id: "crroute_c9t4xoy7fmiq7equtu20",
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  managerId: "<id>",
  resourceId: "<id>",
  resourceRevision: "<value>",
  desiredRevision: 198762,
  appliedRevision: 692052,
  status: "revoking",
  lastVerifiedAt: null,
  lastError: "<value>",
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          | Example                                                                                              |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `id`                                                                                                 | *string*                                                                                             | :heavy_check_mark:                                                                                   | Unique identifier for the container registry route.                                                  | crroute_c9t4xoy7fmiq7equtu20                                                                         |
| `deploymentId`                                                                                       | *string*                                                                                             | :heavy_check_mark:                                                                                   | Unique identifier for the deployment.                                                                | dep_0c29fq4a2yjb7kx3smwdgxlc                                                                         |
| `managerId`                                                                                          | *string*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |                                                                                                      |
| `resourceId`                                                                                         | *string*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |                                                                                                      |
| `resourceRevision`                                                                                   | *string*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |                                                                                                      |
| `desiredRevision`                                                                                    | *number*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |                                                                                                      |
| `appliedRevision`                                                                                    | *number*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |                                                                                                      |
| `status`                                                                                             | [operations.VerifyContainerRegistryStatus](../../models/operations/verifycontainerregistrystatus.md) | :heavy_check_mark:                                                                                   | N/A                                                                                                  |                                                                                                      |
| `lastVerifiedAt`                                                                                     | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)        | :heavy_check_mark:                                                                                   | N/A                                                                                                  |                                                                                                      |
| `lastError`                                                                                          | *string*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |                                                                                                      |