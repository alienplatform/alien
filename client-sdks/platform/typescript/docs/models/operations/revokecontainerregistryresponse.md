# RevokeContainerRegistryResponse

Registry revocation requested

## Example Usage

```typescript
import { RevokeContainerRegistryResponse } from "@alienplatform/platform-api/models/operations";

let value: RevokeContainerRegistryResponse = {
  id: "crroute_c9t4xoy7fmiq7equtu20",
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  managerId: "<id>",
  resourceId: "<id>",
  resourceRevision: "<value>",
  desiredRevision: 624510,
  appliedRevision: 9616,
  status: "applying",
  lastVerifiedAt: new Date("2026-09-21T16:28:46.372Z"),
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
| `status`                                                                                             | [operations.RevokeContainerRegistryStatus](../../models/operations/revokecontainerregistrystatus.md) | :heavy_check_mark:                                                                                   | N/A                                                                                                  |                                                                                                      |
| `lastVerifiedAt`                                                                                     | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)        | :heavy_check_mark:                                                                                   | N/A                                                                                                  |                                                                                                      |
| `lastError`                                                                                          | *string*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |                                                                                                      |