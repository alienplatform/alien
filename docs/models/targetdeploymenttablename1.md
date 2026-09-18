# TargetDeploymentTableName1

## Example Usage

```typescript
import { TargetDeploymentTableName1 } from "@alienplatform/platform-api/models";

let value: TargetDeploymentTableName1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                    | [models.TargetDeploymentTableNameSecretRef1](../models/targetdeploymenttablenamesecretref1.md) | :heavy_check_mark:                                                                             | Reference to a Kubernetes Secret                                                               |