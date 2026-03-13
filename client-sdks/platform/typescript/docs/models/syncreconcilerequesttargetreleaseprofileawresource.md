# SyncReconcileRequestTargetReleaseProfileAwResource

AWS-specific binding specification

## Example Usage

```typescript
import { SyncReconcileRequestTargetReleaseProfileAwResource } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestTargetReleaseProfileAwResource = {
  resources: [],
};
```

## Fields

| Field                                              | Type                                               | Required                                           | Description                                        |
| -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- |
| `condition`                                        | Record<string, Record<string, *string*>>           | :heavy_minus_sign:                                 | Optional condition for additional filtering (rare) |
| `resources`                                        | *string*[]                                         | :heavy_check_mark:                                 | Resource ARNs to bind to                           |