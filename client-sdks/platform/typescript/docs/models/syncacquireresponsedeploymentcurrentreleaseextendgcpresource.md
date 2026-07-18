# SyncAcquireResponseDeploymentCurrentReleaseExtendGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentCurrentReleaseExtendGcpResource } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentCurrentReleaseExtendGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `condition`                                                                      | *models.SyncAcquireResponseDeploymentCurrentReleaseExtendResourceConditionUnion* | :heavy_minus_sign:                                                               | N/A                                                                              |
| `scope`                                                                          | *string*                                                                         | :heavy_check_mark:                                                               | Scope (project/resource level)                                                   |