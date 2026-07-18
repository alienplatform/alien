# SyncAcquireResponseDeploymentDatabase5

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentDatabase5 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentDatabase5 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                            | [models.SyncAcquireResponseDeploymentDatabaseSecretRef5](../models/syncacquireresponsedeploymentdatabasesecretref5.md) | :heavy_check_mark:                                                                                                     | Reference to a Kubernetes Secret                                                                                       |