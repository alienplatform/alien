# TargetDeploymentDatabaseId

## Example Usage

```typescript
import { TargetDeploymentDatabaseId } from "@alienplatform/platform-api/models";

let value: TargetDeploymentDatabaseId = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                    | [models.TargetDeploymentDatabaseIdSecretRef](../models/targetdeploymentdatabaseidsecretref.md) | :heavy_check_mark:                                                                             | Reference to a Kubernetes Secret                                                               |