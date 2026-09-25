# PublishChildDeploymentReleaseRequest

## Example Usage

```typescript
import { PublishChildDeploymentReleaseRequest } from "@alienplatform/platform-api/models/operations";

let value: PublishChildDeploymentReleaseRequest = {
  id: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  childId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  idempotencyKey: "<value>",
  ifMatch: "rel_WbhQgksrawSKIpEN0NAssHX9",
};
```

## Fields

| Field                                                                                 | Type                                                                                  | Required                                                                              | Description                                                                           | Example                                                                               |
| ------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- |
| `id`                                                                                  | *string*                                                                              | :heavy_check_mark:                                                                    | Unique identifier for the deployment.                                                 | dep_0c29fq4a2yjb7kx3smwdgxlc                                                          |
| `childId`                                                                             | *string*                                                                              | :heavy_check_mark:                                                                    | Unique identifier for the deployment.                                                 | dep_0c29fq4a2yjb7kx3smwdgxlc                                                          |
| `idempotencyKey`                                                                      | *string*                                                                              | :heavy_check_mark:                                                                    | N/A                                                                                   |                                                                                       |
| `ifMatch`                                                                             | *string*                                                                              | :heavy_check_mark:                                                                    | Unique identifier for the release.                                                    | rel_WbhQgksrawSKIpEN0NAssHX9                                                          |
| `publishChildDeploymentRequest`                                                       | [models.PublishChildDeploymentRequest](../../models/publishchilddeploymentrequest.md) | :heavy_minus_sign:                                                                    | N/A                                                                                   |                                                                                       |