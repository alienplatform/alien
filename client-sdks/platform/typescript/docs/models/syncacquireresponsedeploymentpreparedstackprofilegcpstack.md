# SyncAcquireResponseDeploymentPreparedStackProfileGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentPreparedStackProfileGcpStack } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentPreparedStackProfileGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                                         | Type                                                                          | Required                                                                      | Description                                                                   |
| ----------------------------------------------------------------------------- | ----------------------------------------------------------------------------- | ----------------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| `condition`                                                                   | *models.SyncAcquireResponseDeploymentPreparedStackProfileStackConditionUnion* | :heavy_minus_sign:                                                            | N/A                                                                           |
| `scope`                                                                       | *string*                                                                      | :heavy_check_mark:                                                            | Scope (project/resource level)                                                |