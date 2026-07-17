# SyncAcquireResponseDeploymentUsername2

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentUsername2 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentUsername2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                            | [models.SyncAcquireResponseDeploymentUsernameSecretRef2](../models/syncacquireresponsedeploymentusernamesecretref2.md) | :heavy_check_mark:                                                                                                     | Reference to a Kubernetes Secret                                                                                       |