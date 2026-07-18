# SyncAcquireResponseDeploymentProjectId

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentProjectId } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentProjectId = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                            | [models.SyncAcquireResponseDeploymentProjectIdSecretRef](../models/syncacquireresponsedeploymentprojectidsecretref.md) | :heavy_check_mark:                                                                                                     | Reference to a Kubernetes Secret                                                                                       |