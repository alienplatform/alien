# SyncAcquireResponseDeploymentPreparedStackManagement2

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentPreparedStackManagement2 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentPreparedStackManagement2 = {
  override: {},
};
```

## Fields

| Field                                                                                                                             | Type                                                                                                                              | Required                                                                                                                          | Description                                                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `override`                                                                                                                        | Record<string, *models.SyncAcquireResponseDeploymentPreparedStackOverrideUnion*[]>                                                | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |