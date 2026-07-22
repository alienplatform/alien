# DeploymentPendingPreparedStackManagement2

## Example Usage

```typescript
import { DeploymentPendingPreparedStackManagement2 } from "@alienplatform/platform-api/models";

let value: DeploymentPendingPreparedStackManagement2 = {
  override: {
    "key": [
      {
        description:
          "midst clamor untrue request onset eek above gosh likewise milky",
        id: "<id>",
        platforms: {},
      },
    ],
    "key1": [],
    "key2": [
      {
        description:
          "midst clamor untrue request onset eek above gosh likewise milky",
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
| `override`                                                                                                                        | Record<string, *models.DeploymentPendingPreparedStackOverrideUnion*[]>                                                            | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |
