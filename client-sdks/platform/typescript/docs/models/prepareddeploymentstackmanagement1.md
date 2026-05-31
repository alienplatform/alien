# PreparedDeploymentStackManagement1

## Example Usage

```typescript
import { PreparedDeploymentStackManagement1 } from "@alienplatform/platform-api/models";

let value: PreparedDeploymentStackManagement1 = {
  extend: {
    "key": [
      {
        description:
          "gladly creative where youthfully likewise improbable fabricate hence gadzooks",
        id: "<id>",
        platforms: {},
      },
    ],
    "key1": [],
  },
};
```

## Fields

| Field                                                                                                                             | Type                                                                                                                              | Required                                                                                                                          | Description                                                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `extend`                                                                                                                          | Record<string, *models.PreparedDeploymentStackExtendUnion*[]>                                                                     | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |