# SyncAcquireResponseDeploymentUsername5

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentUsername5 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentUsername5 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                            | [models.SyncAcquireResponseDeploymentUsernameSecretRef5](../models/syncacquireresponsedeploymentusernamesecretref5.md) | :heavy_check_mark:                                                                                                     | Reference to a Kubernetes Secret                                                                                       |