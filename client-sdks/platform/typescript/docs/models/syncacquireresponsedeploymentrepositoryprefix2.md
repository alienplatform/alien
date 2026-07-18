# SyncAcquireResponseDeploymentRepositoryPrefix2

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentRepositoryPrefix2 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentRepositoryPrefix2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                                  | Type                                                                                                                                   | Required                                                                                                                               | Description                                                                                                                            |
| -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                                            | [models.SyncAcquireResponseDeploymentRepositoryPrefixSecretRef2](../models/syncacquireresponsedeploymentrepositoryprefixsecretref2.md) | :heavy_check_mark:                                                                                                                     | Reference to a Kubernetes Secret                                                                                                       |