# SyncAcquireResponseDeploymentCurrentReleaseProfileGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentCurrentReleaseProfileGcpResource } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentCurrentReleaseProfileGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                                             | Type                                                                              | Required                                                                          | Description                                                                       |
| --------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- |
| `condition`                                                                       | *models.SyncAcquireResponseDeploymentCurrentReleaseProfileResourceConditionUnion* | :heavy_minus_sign:                                                                | N/A                                                                               |
| `scope`                                                                           | *string*                                                                          | :heavy_check_mark:                                                                | Scope (project/resource level)                                                    |