# DeploymentPendingPreparedStackManagement1

## Example Usage

```typescript
import { DeploymentPendingPreparedStackManagement1 } from "@alienplatform/platform-api/models";

let value: DeploymentPendingPreparedStackManagement1 = {
  extend: {
    "key": [],
    "key1": [
      "<value>",
    ],
    "key2": [],
  },
};
```

## Fields

| Field                                                                                                                             | Type                                                                                                                              | Required                                                                                                                          | Description                                                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `extend`                                                                                                                          | Record<string, *models.DeploymentPendingPreparedStackExtendUnion*[]>                                                              | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |
