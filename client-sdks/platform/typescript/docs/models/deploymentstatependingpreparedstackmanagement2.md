# DeploymentStatePendingPreparedStackManagement2

## Example Usage

```typescript
import { DeploymentStatePendingPreparedStackManagement2 } from "@alienplatform/platform-api/models";

let value: DeploymentStatePendingPreparedStackManagement2 = {
  override: {
    "key": [
      {
        description: "furthermore courageously lazily approach speedily",
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
| `override`                                                                                                                        | Record<string, *models.DeploymentStatePendingPreparedStackOverrideUnion*[]>                                                       | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |