# PreparedDeploymentStackManagement2

## Example Usage

```typescript
import { PreparedDeploymentStackManagement2 } from "@alienplatform/platform-api/models";

let value: PreparedDeploymentStackManagement2 = {
  override: {
    "key": [],
    "key1": [
      {
        description: "candid unless excluding avow low accurate nor on",
        id: "<id>",
        platforms: {},
      },
    ],
    "key2": [],
  },
};
```

## Fields

| Field                                                                                                                             | Type                                                                                                                              | Required                                                                                                                          | Description                                                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `override`                                                                                                                        | Record<string, *models.PreparedDeploymentStackOverrideUnion*[]>                                                                   | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |