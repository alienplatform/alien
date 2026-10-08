# ResolveResponseInstallContext

Target install context derived from platform-managed manager metadata. Present for cloud push platforms.

## Example Usage

```typescript
import { ResolveResponseInstallContext } from "@alienplatform/platform-api/models";

let value: ResolveResponseInstallContext = {
  platform: "kubernetes",
  managementConfig: {
    serviceAccountEmail: "<value>",
    platform: "gcp",
  },
};
```

## Fields

| Field                                                                                                                                                                                                       | Type                                                                                                                                                                                                        | Required                                                                                                                                                                                                    | Description                                                                                                                                                                                                 |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `platform`                                                                                                                                                                                                  | [models.ResolveResponsePlatformEnum](../models/resolveresponseplatformenum.md)                                                                                                                              | :heavy_check_mark:                                                                                                                                                                                          | Represents the target cloud platform.                                                                                                                                                                       |
| `managementConfig`                                                                                                                                                                                          | *models.ResolveResponseManagementConfigUnion*                                                                                                                                                               | :heavy_check_mark:                                                                                                                                                                                          | Management configuration for different cloud platforms.<br/><br/>Platform-derived configuration for cross-account/cross-tenant access.<br/>This is NOT user-specified - it's derived from the Manager's ServiceAccount. |