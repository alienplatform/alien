# DeploymentStatePendingPreparedStackManagement1

## Example Usage

```typescript
import { DeploymentStatePendingPreparedStackManagement1 } from "@alienplatform/platform-api/models";

let value: DeploymentStatePendingPreparedStackManagement1 = {
  extend: {},
};
```

## Fields

| Field                                                                                                                             | Type                                                                                                                              | Required                                                                                                                          | Description                                                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `extend`                                                                                                                          | Record<string, *models.DeploymentStatePendingPreparedStackExtendUnion*[]>                                                         | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |