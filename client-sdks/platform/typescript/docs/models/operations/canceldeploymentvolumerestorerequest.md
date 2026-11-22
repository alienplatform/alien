# CancelDeploymentVolumeRestoreRequest

## Example Usage

```typescript
import { CancelDeploymentVolumeRestoreRequest } from "@alienplatform/platform-api/models/operations";

let value: CancelDeploymentVolumeRestoreRequest = {
  id: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  requestId: "vrst_rs1j4dytswpy7wbc819jdcx",
};
```

## Fields

| Field                                     | Type                                      | Required                                  | Description                               | Example                                   |
| ----------------------------------------- | ----------------------------------------- | ----------------------------------------- | ----------------------------------------- | ----------------------------------------- |
| `id`                                      | *string*                                  | :heavy_check_mark:                        | Unique identifier for the deployment.     | dep_0c29fq4a2yjb7kx3smwdgxlc              |
| `requestId`                               | *string*                                  | :heavy_check_mark:                        | Unique identifier for the volume restore. | vrst_rs1j4dytswpy7wbc819jdcx              |