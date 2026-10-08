# TargetDeploymentManagement2

## Example Usage

```typescript
import { TargetDeploymentManagement2 } from "@alienplatform/platform-api/models";

let value: TargetDeploymentManagement2 = {
  override: {
    "key": [],
  },
};
```

## Fields

| Field                                                                                                                             | Type                                                                                                                              | Required                                                                                                                          | Description                                                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `override`                                                                                                                        | Record<string, *models.TargetDeploymentOverrideUnion*[]>                                                                          | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |