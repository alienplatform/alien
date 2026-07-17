# SyncAcquireResponseDeploymentUsername4

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentUsername4 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentUsername4 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                            | [models.SyncAcquireResponseDeploymentUsernameSecretRef4](../models/syncacquireresponsedeploymentusernamesecretref4.md) | :heavy_check_mark:                                                                                                     | Reference to a Kubernetes Secret                                                                                       |