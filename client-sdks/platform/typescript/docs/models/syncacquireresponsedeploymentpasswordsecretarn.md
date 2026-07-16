# SyncAcquireResponseDeploymentPasswordSecretArn

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentPasswordSecretArn } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentPasswordSecretArn = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                                  | Type                                                                                                                                   | Required                                                                                                                               | Description                                                                                                                            |
| -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                                            | [models.SyncAcquireResponseDeploymentPasswordSecretArnSecretRef](../models/syncacquireresponsedeploymentpasswordsecretarnsecretref.md) | :heavy_check_mark:                                                                                                                     | Reference to a Kubernetes Secret                                                                                                       |