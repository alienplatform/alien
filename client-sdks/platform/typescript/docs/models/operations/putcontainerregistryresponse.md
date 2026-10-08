# PutContainerRegistryResponse

Registry route

## Example Usage

```typescript
import { PutContainerRegistryResponse } from "@alienplatform/platform-api/models/operations";

let value: PutContainerRegistryResponse = {
  id: "crroute_c9t4xoy7fmiq7equtu20",
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  managerId: "<id>",
  resourceId: "<id>",
  resourceRevision: "<value>",
  desiredRevision: 462817,
  appliedRevision: 373439,
  status: "degraded",
  lastVerifiedAt: new Date("2024-07-24T04:10:20.609Z"),
  lastError: "<value>",
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    | Example                                                                                        |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `id`                                                                                           | *string*                                                                                       | :heavy_check_mark:                                                                             | Unique identifier for the container registry route.                                            | crroute_c9t4xoy7fmiq7equtu20                                                                   |
| `deploymentId`                                                                                 | *string*                                                                                       | :heavy_check_mark:                                                                             | Unique identifier for the deployment.                                                          | dep_0c29fq4a2yjb7kx3smwdgxlc                                                                   |
| `managerId`                                                                                    | *string*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |                                                                                                |
| `resourceId`                                                                                   | *string*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |                                                                                                |
| `resourceRevision`                                                                             | *string*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |                                                                                                |
| `desiredRevision`                                                                              | *number*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |                                                                                                |
| `appliedRevision`                                                                              | *number*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |                                                                                                |
| `status`                                                                                       | [operations.PutContainerRegistryStatus](../../models/operations/putcontainerregistrystatus.md) | :heavy_check_mark:                                                                             | N/A                                                                                            |                                                                                                |
| `lastVerifiedAt`                                                                               | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)  | :heavy_check_mark:                                                                             | N/A                                                                                            |                                                                                                |
| `lastError`                                                                                    | *string*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |                                                                                                |