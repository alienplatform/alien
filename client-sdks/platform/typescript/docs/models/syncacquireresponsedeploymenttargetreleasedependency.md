# SyncAcquireResponseDeploymentTargetReleaseDependency

Reference to a resource by its stable id and resource type.

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentTargetReleaseDependency } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentTargetReleaseDependency = {
  id: "<id>",
  type: "<value>",
};
```

## Fields

| Field                                                                                                                                                      | Type                                                                                                                                                       | Required                                                                                                                                                   | Description                                                                                                                                                |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `id`                                                                                                                                                       | *string*                                                                                                                                                   | :heavy_check_mark:                                                                                                                                         | N/A                                                                                                                                                        |
| `type`                                                                                                                                                     | *string*                                                                                                                                                   | :heavy_check_mark:                                                                                                                                         | Resource type identifier that determines the specific kind of resource. This field is used for polymorphic deserialization and resource-specific behavior. |