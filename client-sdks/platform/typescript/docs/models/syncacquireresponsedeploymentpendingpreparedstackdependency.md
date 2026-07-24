# SyncAcquireResponseDeploymentPendingPreparedStackDependency

Reference to a resource by its stable id and resource type.

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentPendingPreparedStackDependency } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentPendingPreparedStackDependency = {
  id: "<id>",
  type: "<value>",
};
```

## Fields

| Field                                                                                                                                                      | Type                                                                                                                                                       | Required                                                                                                                                                   | Description                                                                                                                                                |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `id`                                                                                                                                                       | *string*                                                                                                                                                   | :heavy_check_mark:                                                                                                                                         | N/A                                                                                                                                                        |
| `type`                                                                                                                                                     | *string*                                                                                                                                                   | :heavy_check_mark:                                                                                                                                         | Resource type identifier that determines the specific kind of resource. This field is used for polymorphic deserialization and resource-specific behavior. |
