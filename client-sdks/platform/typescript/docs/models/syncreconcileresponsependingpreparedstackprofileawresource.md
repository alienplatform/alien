# SyncReconcileResponsePendingPreparedStackProfileAwResource

AWS-specific binding specification

## Example Usage

```typescript
import { SyncReconcileResponsePendingPreparedStackProfileAwResource } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponsePendingPreparedStackProfileAwResource = {
  resources: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
};
```

## Fields

| Field                                              | Type                                               | Required                                           | Description                                        |
| -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- |
| `condition`                                        | Record<string, Record<string, *string*>>           | :heavy_minus_sign:                                 | Optional condition for additional filtering (rare) |
| `resources`                                        | *string*[]                                         | :heavy_check_mark:                                 | Resource ARNs to bind to                           |
