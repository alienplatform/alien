# DeploymentPreparedStackManagement1

## Example Usage

```typescript
import { DeploymentPreparedStackManagement1 } from "@alienplatform/platform-api/models";

let value: DeploymentPreparedStackManagement1 = {
  extend: {
    "key": [
      "<value>",
    ],
    "key1": [
      {
        description: "netsuke delectable recklessly dramatic brr for",
        id: "<id>",
        platforms: {},
      },
    ],
    "key2": [
      "<value>",
    ],
  },
};
```

## Fields

| Field                                                                                                                             | Type                                                                                                                              | Required                                                                                                                          | Description                                                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `extend`                                                                                                                          | Record<string, *models.DeploymentPreparedStackExtendUnion*[]>                                                                     | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |
