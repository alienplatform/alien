# SyncAcquireResponseDeploymentPreparedStackOverrideGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentPreparedStackOverrideGcpResource } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentPreparedStackOverrideGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                                             | Type                                                                              | Required                                                                          | Description                                                                       |
| --------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- |
| `condition`                                                                       | *models.SyncAcquireResponseDeploymentPreparedStackOverrideResourceConditionUnion* | :heavy_minus_sign:                                                                | N/A                                                                               |
| `scope`                                                                           | *string*                                                                          | :heavy_check_mark:                                                                | Scope (project/resource level)                                                    |