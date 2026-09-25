# PublishChildDeploymentRequestManagement2

## Example Usage

```typescript
import { PublishChildDeploymentRequestManagement2 } from "@alienplatform/platform-api/models";

let value: PublishChildDeploymentRequestManagement2 = {
  override: {
    "key": [
      "<value>",
    ],
    "key1": [],
    "key2": [],
  },
};
```

## Fields

| Field                                                                                                                             | Type                                                                                                                              | Required                                                                                                                          | Description                                                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `override`                                                                                                                        | Record<string, *models.PublishChildDeploymentRequestOverrideUnion*[]>                                                             | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |