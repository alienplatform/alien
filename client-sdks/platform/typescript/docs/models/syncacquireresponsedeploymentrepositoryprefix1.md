# SyncAcquireResponseDeploymentRepositoryPrefix1

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentRepositoryPrefix1 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentRepositoryPrefix1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                                  | Type                                                                                                                                   | Required                                                                                                                               | Description                                                                                                                            |
| -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                                            | [models.SyncAcquireResponseDeploymentRepositoryPrefixSecretRef1](../models/syncacquireresponsedeploymentrepositoryprefixsecretref1.md) | :heavy_check_mark:                                                                                                                     | Reference to a Kubernetes Secret                                                                                                       |