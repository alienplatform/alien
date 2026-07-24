# PersistImportedDeploymentRequestPendingPreparedStackManagement2

## Example Usage

```typescript
import { PersistImportedDeploymentRequestPendingPreparedStackManagement2 } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestPendingPreparedStackManagement2 = {
  override: {
    "key": [],
    "key1": [
      "<value>",
    ],
    "key2": [],
  },
};
```

## Fields

| Field                                                                                                                             | Type                                                                                                                              | Required                                                                                                                          | Description                                                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `override`                                                                                                                        | Record<string, *models.PersistImportedDeploymentRequestPendingPreparedStackOverrideUnion*[]>                                      | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |
