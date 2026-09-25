# CreateChildDeploymentRequestManagement1

## Example Usage

```typescript
import { CreateChildDeploymentRequestManagement1 } from "@alienplatform/platform-api/models";

let value: CreateChildDeploymentRequestManagement1 = {
  extend: {},
};
```

## Fields

| Field                                                                                                                             | Type                                                                                                                              | Required                                                                                                                          | Description                                                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `extend`                                                                                                                          | Record<string, *models.CreateChildDeploymentRequestExtendUnion*[]>                                                                | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |