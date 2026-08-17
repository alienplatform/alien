# TargetDeploymentResourceId

## Example Usage

```typescript
import { TargetDeploymentResourceId } from "@alienplatform/platform-api/models";

let value: TargetDeploymentResourceId = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                    | [models.TargetDeploymentResourceIdSecretRef](../models/targetdeploymentresourceidsecretref.md) | :heavy_check_mark:                                                                             | Reference to a Kubernetes Secret                                                               |