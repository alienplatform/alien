# SyncReconcileRequestCurrentReleaseOverrideAwStack

AWS-specific binding specification

## Example Usage

```typescript
import { SyncReconcileRequestCurrentReleaseOverrideAwStack } from "@aliendotdev/platform-api/models";

let value: SyncReconcileRequestCurrentReleaseOverrideAwStack = {
  resources: [
    "<value 1>",
    "<value 2>",
  ],
};
```

## Fields

| Field                                              | Type                                               | Required                                           | Description                                        |
| -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- |
| `condition`                                        | Record<string, Record<string, *string*>>           | :heavy_minus_sign:                                 | Optional condition for additional filtering (rare) |
| `resources`                                        | *string*[]                                         | :heavy_check_mark:                                 | Resource ARNs to bind to                           |