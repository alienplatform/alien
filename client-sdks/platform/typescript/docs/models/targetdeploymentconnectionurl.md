# TargetDeploymentConnectionUrl

## Example Usage

```typescript
import { TargetDeploymentConnectionUrl } from "@alienplatform/platform-api/models";

let value: TargetDeploymentConnectionUrl = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                          | [models.TargetDeploymentConnectionUrlSecretRef](../models/targetdeploymentconnectionurlsecretref.md) | :heavy_check_mark:                                                                                   | Reference to a Kubernetes Secret                                                                     |