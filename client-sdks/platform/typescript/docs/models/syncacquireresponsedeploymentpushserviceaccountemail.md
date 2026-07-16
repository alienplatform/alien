# SyncAcquireResponseDeploymentPushServiceAccountEmail

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentPushServiceAccountEmail } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentPushServiceAccountEmail = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                                              | Type                                                                                                                                               | Required                                                                                                                                           | Description                                                                                                                                        |
| -------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                                                        | [models.SyncAcquireResponseDeploymentPushServiceAccountEmailSecretRef](../models/syncacquireresponsedeploymentpushserviceaccountemailsecretref.md) | :heavy_check_mark:                                                                                                                                 | Reference to a Kubernetes Secret                                                                                                                   |