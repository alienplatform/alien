# SyncReconcileResponse

State reconciliation result with optional target

## Example Usage

```typescript
import { SyncReconcileResponse } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponse = {
  success: false,
  current: {
    platform: "aws",
    status: "update-failed",
  },
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `success`                                                                        | *boolean*                                                                        | :heavy_check_mark:                                                               | Whether the state was reconciled                                                 |
| `current`                                                                        | [models.SyncReconcileResponseCurrent](../models/syncreconcileresponsecurrent.md) | :heavy_check_mark:                                                               | Current deployment state after reconciliation                                    |
| `target`                                                                         | [models.SyncReconcileResponseTarget](../models/syncreconcileresponsetarget.md)   | :heavy_minus_sign:                                                               | Target deployment if update is needed                                            |