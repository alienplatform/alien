# PersistImportedDeploymentRequestPreparedStackManagement2

## Example Usage

```typescript
import { PersistImportedDeploymentRequestPreparedStackManagement2 } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestPreparedStackManagement2 = {
  override: {
    "key": [],
  },
};
```

## Fields

| Field                                                                                                                             | Type                                                                                                                              | Required                                                                                                                          | Description                                                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `override`                                                                                                                        | Record<string, *models.PersistImportedDeploymentRequestPreparedStackOverrideUnion*[]>                                             | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |
