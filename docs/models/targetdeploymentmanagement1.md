# TargetDeploymentManagement1

## Example Usage

```typescript
import { TargetDeploymentManagement1 } from "@alienplatform/platform-api/models";

let value: TargetDeploymentManagement1 = {
  extend: {
    "key": [],
    "key1": [],
  },
};
```

## Fields

| Field                                                                                                                             | Type                                                                                                                              | Required                                                                                                                          | Description                                                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `extend`                                                                                                                          | Record<string, *models.TargetDeploymentExtendUnion*[]>                                                                            | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |