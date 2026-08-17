# DeploymentStatePreparedStackManagement1

## Example Usage

```typescript
import { DeploymentStatePreparedStackManagement1 } from "@alienplatform/platform-api/models";

let value: DeploymentStatePreparedStackManagement1 = {
  extend: {
    "key": [
      "<value>",
    ],
    "key1": [],
  },
};
```

## Fields

| Field                                                                                                                             | Type                                                                                                                              | Required                                                                                                                          | Description                                                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `extend`                                                                                                                          | Record<string, *models.DeploymentStatePreparedStackExtendUnion*[]>                                                                | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |