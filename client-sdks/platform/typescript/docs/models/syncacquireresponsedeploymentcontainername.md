# SyncAcquireResponseDeploymentContainerName

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentContainerName } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentContainerName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                          | Type                                                                                                                           | Required                                                                                                                       | Description                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                                    | [models.SyncAcquireResponseDeploymentContainerNameSecretRef](../models/syncacquireresponsedeploymentcontainernamesecretref.md) | :heavy_check_mark:                                                                                                             | Reference to a Kubernetes Secret                                                                                               |