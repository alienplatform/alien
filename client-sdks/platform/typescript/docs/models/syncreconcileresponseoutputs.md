# SyncReconcileResponseOutputs

Resource outputs that can hold output data for any resource type in the Alien system. All resource outputs share a common 'type' field with additional type-specific output properties.

## Example Usage

```typescript
import { SyncReconcileResponseOutputs } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseOutputs = {
  type: "<value>",
};
```

## Fields

| Field                                                                                                                                                      | Type                                                                                                                                                       | Required                                                                                                                                                   | Description                                                                                                                                                |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `type`                                                                                                                                                     | *string*                                                                                                                                                   | :heavy_check_mark:                                                                                                                                         | Resource type identifier that determines the specific kind of resource. This field is used for polymorphic deserialization and resource-specific behavior. |
| `additionalProperties`                                                                                                                                     | Record<string, *any*>                                                                                                                                      | :heavy_minus_sign:                                                                                                                                         | N/A                                                                                                                                                        |