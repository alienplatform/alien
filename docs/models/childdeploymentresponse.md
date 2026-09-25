# ChildDeploymentResponse

## Example Usage

```typescript
import { ChildDeploymentResponse } from "@alienplatform/platform-api/models";

let value: ChildDeploymentResponse = {
  projectId: "<id>",
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  parentDeploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
  desiredReleaseId: "<id>",
  currentReleaseId: "<id>",
  name: "<value>",
  status: "<value>",
  internalServices: [],
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              | Example                                                  |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `projectId`                                              | *string*                                                 | :heavy_check_mark:                                       | N/A                                                      |                                                          |
| `deploymentId`                                           | *string*                                                 | :heavy_check_mark:                                       | Unique identifier for the deployment.                    | dep_0c29fq4a2yjb7kx3smwdgxlc                             |
| `parentDeploymentId`                                     | *string*                                                 | :heavy_check_mark:                                       | Unique identifier for the deployment.                    | dep_0c29fq4a2yjb7kx3smwdgxlc                             |
| `releaseId`                                              | *string*                                                 | :heavy_check_mark:                                       | Unique identifier for the release.                       | rel_WbhQgksrawSKIpEN0NAssHX9                             |
| `desiredReleaseId`                                       | *string*                                                 | :heavy_check_mark:                                       | N/A                                                      |                                                          |
| `currentReleaseId`                                       | *string*                                                 | :heavy_check_mark:                                       | N/A                                                      |                                                          |
| `name`                                                   | *string*                                                 | :heavy_check_mark:                                       | N/A                                                      |                                                          |
| `status`                                                 | *string*                                                 | :heavy_check_mark:                                       | N/A                                                      |                                                          |
| `internalServices`                                       | [models.InternalService](../models/internalservice.md)[] | :heavy_check_mark:                                       | N/A                                                      |                                                          |