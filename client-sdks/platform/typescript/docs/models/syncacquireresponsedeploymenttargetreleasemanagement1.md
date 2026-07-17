# SyncAcquireResponseDeploymentTargetReleaseManagement1

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentTargetReleaseManagement1 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentTargetReleaseManagement1 = {
  extend: {
    "key": [
      "<value>",
    ],
  },
};
```

## Fields

| Field                                                                                                                             | Type                                                                                                                              | Required                                                                                                                          | Description                                                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `extend`                                                                                                                          | Record<string, *models.SyncAcquireResponseDeploymentTargetReleaseExtendUnion*[]>                                                  | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |