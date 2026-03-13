# DeploymentDetailResponseManagement2

## Example Usage

```typescript
import { DeploymentDetailResponseManagement2 } from "@aliendotdev/platform-api/models";

let value: DeploymentDetailResponseManagement2 = {
  override: {
    "key": [
      {
        description: "besides meaningfully genuine",
        id: "<id>",
        platforms: {},
      },
    ],
    "key1": [
      {
        description: "besides meaningfully genuine",
        id: "<id>",
        platforms: {},
      },
    ],
  },
};
```

## Fields

| Field                                                                                                                             | Type                                                                                                                              | Required                                                                                                                          | Description                                                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `override`                                                                                                                        | Record<string, *models.DeploymentDetailResponseOverrideUnion*[]>                                                                  | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |