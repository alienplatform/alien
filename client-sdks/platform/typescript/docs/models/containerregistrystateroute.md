# ContainerRegistryStateRoute

## Example Usage

```typescript
import { ContainerRegistryStateRoute } from "@alienplatform/platform-api/models";

let value: ContainerRegistryStateRoute = {
  id: "crroute_c9t4xoy7fmiq7equtu20",
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  managerId: "<id>",
  resourceId: "<id>",
  resourceRevision: "<value>",
  desiredRevision: 667649,
  appliedRevision: 376748,
  status: "resolving",
  lastVerifiedAt: new Date("2026-11-24T20:30:53.788Z"),
  lastError: "<value>",
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   | Example                                                                                       |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `id`                                                                                          | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the container registry route.                                           | crroute_c9t4xoy7fmiq7equtu20                                                                  |
| `deploymentId`                                                                                | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the deployment.                                                         | dep_0c29fq4a2yjb7kx3smwdgxlc                                                                  |
| `managerId`                                                                                   | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `resourceId`                                                                                  | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `resourceRevision`                                                                            | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `desiredRevision`                                                                             | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `appliedRevision`                                                                             | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `status`                                                                                      | [models.ContainerRegistryStateRouteStatus](../models/containerregistrystateroutestatus.md)    | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `lastVerifiedAt`                                                                              | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `lastError`                                                                                   | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |