# DeploymentDetailResponsePreparedStackManagement1

## Example Usage

```typescript
import { DeploymentDetailResponsePreparedStackManagement1 } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponsePreparedStackManagement1 = {
  extend: {
    "key": [],
  },
};
```

## Fields

| Field                                                                                                                             | Type                                                                                                                              | Required                                                                                                                          | Description                                                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `extend`                                                                                                                          | Record<string, *models.DeploymentDetailResponsePreparedStackExtendUnion*[]>                                                       | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |
