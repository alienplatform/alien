# PersistImportedDeploymentRequestManagement1

## Example Usage

```typescript
import { PersistImportedDeploymentRequestManagement1 } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestManagement1 = {
  extend: {
    "key": [
      "<value>",
    ],
    "key1": [],
  },
};
```

## Fields

| Field                                                                                                                             | Type                                                                                                                              | Required                                                                                                                          | Description                                                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `extend`                                                                                                                          | Record<string, *models.PersistImportedDeploymentRequestExtendUnion*[]>                                                            | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |