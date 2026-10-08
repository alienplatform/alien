# DeploymentDetailResponsePendingPreparedStackManagement2

## Example Usage

```typescript
import { DeploymentDetailResponsePendingPreparedStackManagement2 } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponsePendingPreparedStackManagement2 = {
  override: {
    "key": [
      {
        description: "hippodrome around after pfft primary afore creamy",
        id: "<id>",
        platforms: {},
      },
    ],
    "key1": [
      "<value>",
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
| `override`                                                                                                                        | Record<string, *models.DeploymentDetailResponsePendingPreparedStackOverrideUnion*[]>                                              | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |
