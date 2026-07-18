# SyncAcquireResponseDeploymentPreparedStackExtendGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentPreparedStackExtendGcpStack } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentPreparedStackExtendGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `condition`                                                                  | *models.SyncAcquireResponseDeploymentPreparedStackExtendStackConditionUnion* | :heavy_minus_sign:                                                           | N/A                                                                          |
| `scope`                                                                      | *string*                                                                     | :heavy_check_mark:                                                           | Scope (project/resource level)                                               |