# DeploymentDetailResponsePreparedStackManagement2

## Example Usage

```typescript
import { DeploymentDetailResponsePreparedStackManagement2 } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponsePreparedStackManagement2 = {
  override: {
    "key": [
      "<value>",
    ],
    "key1": [],
    "key2": [
      {
        description: "beneath self-confidence abaft gah whether",
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
| `override`                                                                                                                        | Record<string, *models.DeploymentDetailResponsePreparedStackOverrideUnion*[]>                                                     | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |
