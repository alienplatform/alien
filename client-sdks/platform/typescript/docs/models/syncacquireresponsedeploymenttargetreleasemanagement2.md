# SyncAcquireResponseDeploymentTargetReleaseManagement2

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentTargetReleaseManagement2 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentTargetReleaseManagement2 = {
  override: {
    "key": [
      "<value>",
    ],
    "key1": [
      "<value>",
    ],
  },
};
```

## Fields

| Field                                                                                                                             | Type                                                                                                                              | Required                                                                                                                          | Description                                                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `override`                                                                                                                        | Record<string, *models.SyncAcquireResponseDeploymentTargetReleaseOverrideUnion*[]>                                                | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |