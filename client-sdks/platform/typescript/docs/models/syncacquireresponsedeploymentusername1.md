# SyncAcquireResponseDeploymentUsername1

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentUsername1 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentUsername1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                            | [models.SyncAcquireResponseDeploymentUsernameSecretRef1](../models/syncacquireresponsedeploymentusernamesecretref1.md) | :heavy_check_mark:                                                                                                     | Reference to a Kubernetes Secret                                                                                       |