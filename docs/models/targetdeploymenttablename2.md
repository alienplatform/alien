# TargetDeploymentTableName2

## Example Usage

```typescript
import { TargetDeploymentTableName2 } from "@alienplatform/platform-api/models";

let value: TargetDeploymentTableName2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                    | [models.TargetDeploymentTableNameSecretRef2](../models/targetdeploymenttablenamesecretref2.md) | :heavy_check_mark:                                                                             | Reference to a Kubernetes Secret                                                               |