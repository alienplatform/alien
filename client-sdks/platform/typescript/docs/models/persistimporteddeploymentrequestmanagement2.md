# PersistImportedDeploymentRequestManagement2

## Example Usage

```typescript
import { PersistImportedDeploymentRequestManagement2 } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestManagement2 = {
  override: {
    "key": [
      {
        description: "rebound lay anti access underneath",
        id: "<id>",
        platforms: {},
      },
    ],
    "key1": [
      {
        description: "rebound lay anti access underneath",
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
| `override`                                                                                                                        | Record<string, *models.PersistImportedDeploymentRequestOverrideUnion*[]>                                                          | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |