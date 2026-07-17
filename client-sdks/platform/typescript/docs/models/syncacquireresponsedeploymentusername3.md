# SyncAcquireResponseDeploymentUsername3

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentUsername3 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentUsername3 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                            | [models.SyncAcquireResponseDeploymentUsernameSecretRef3](../models/syncacquireresponsedeploymentusernamesecretref3.md) | :heavy_check_mark:                                                                                                     | Reference to a Kubernetes Secret                                                                                       |