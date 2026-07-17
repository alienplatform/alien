# SyncAcquireResponseDeploymentTargetReleaseProfileGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentTargetReleaseProfileGcpResource } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentTargetReleaseProfileGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `condition`                                                                      | *models.SyncAcquireResponseDeploymentTargetReleaseProfileResourceConditionUnion* | :heavy_minus_sign:                                                               | N/A                                                                              |
| `scope`                                                                          | *string*                                                                         | :heavy_check_mark:                                                               | Scope (project/resource level)                                                   |