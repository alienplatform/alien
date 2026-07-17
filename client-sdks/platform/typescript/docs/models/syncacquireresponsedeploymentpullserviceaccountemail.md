# SyncAcquireResponseDeploymentPullServiceAccountEmail

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentPullServiceAccountEmail } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentPullServiceAccountEmail = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                                              | Type                                                                                                                                               | Required                                                                                                                                           | Description                                                                                                                                        |
| -------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                                                        | [models.SyncAcquireResponseDeploymentPullServiceAccountEmailSecretRef](../models/syncacquireresponsedeploymentpullserviceaccountemailsecretref.md) | :heavy_check_mark:                                                                                                                                 | Reference to a Kubernetes Secret                                                                                                                   |