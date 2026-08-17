# TargetDeploymentProjectId

## Example Usage

```typescript
import { TargetDeploymentProjectId } from "@alienplatform/platform-api/models";

let value: TargetDeploymentProjectId = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                  | [models.TargetDeploymentProjectIdSecretRef](../models/targetdeploymentprojectidsecretref.md) | :heavy_check_mark:                                                                           | Reference to a Kubernetes Secret                                                             |