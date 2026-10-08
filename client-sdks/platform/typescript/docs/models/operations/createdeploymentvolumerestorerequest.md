# CreateDeploymentVolumeRestoreRequest

## Example Usage

```typescript
import { CreateDeploymentVolumeRestoreRequest } from "@alienplatform/platform-api/models/operations";

let value: CreateDeploymentVolumeRestoreRequest = {
  id: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  createVolumeRestoreRequest: {
    resourceId: "<id>",
    ordinal: 366544,
    snapshotId: "<id>",
  },
};
```

## Fields

| Field                                                                           | Type                                                                            | Required                                                                        | Description                                                                     | Example                                                                         |
| ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| `id`                                                                            | *string*                                                                        | :heavy_check_mark:                                                              | Unique identifier for the deployment.                                           | dep_0c29fq4a2yjb7kx3smwdgxlc                                                    |
| `createVolumeRestoreRequest`                                                    | [models.CreateVolumeRestoreRequest](../../models/createvolumerestorerequest.md) | :heavy_check_mark:                                                              | N/A                                                                             |                                                                                 |
