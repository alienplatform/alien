# SyncAcquireResponseDeploymentPreparedStackExtendGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentPreparedStackExtendGcpResource } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentPreparedStackExtendGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                                           | Type                                                                            | Required                                                                        | Description                                                                     |
| ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| `condition`                                                                     | *models.SyncAcquireResponseDeploymentPreparedStackExtendResourceConditionUnion* | :heavy_minus_sign:                                                              | N/A                                                                             |
| `scope`                                                                         | *string*                                                                        | :heavy_check_mark:                                                              | Scope (project/resource level)                                                  |