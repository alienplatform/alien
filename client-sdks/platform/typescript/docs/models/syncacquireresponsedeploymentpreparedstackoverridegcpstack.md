# SyncAcquireResponseDeploymentPreparedStackOverrideGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentPreparedStackOverrideGcpStack } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentPreparedStackOverrideGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `condition`                                                                    | *models.SyncAcquireResponseDeploymentPreparedStackOverrideStackConditionUnion* | :heavy_minus_sign:                                                             | N/A                                                                            |
| `scope`                                                                        | *string*                                                                       | :heavy_check_mark:                                                             | Scope (project/resource level)                                                 |