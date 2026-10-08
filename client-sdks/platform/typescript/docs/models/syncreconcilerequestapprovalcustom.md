# SyncReconcileRequestApprovalCustom

## Example Usage

```typescript
import { SyncReconcileRequestApprovalCustom } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestApprovalCustom = {
  decision: "manual",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `decision`                                                                                                           | [models.SyncReconcileRequestCustomDecision](../models/syncreconcilerequestcustomdecision.md)                         | :heavy_check_mark:                                                                                                   | Whether matching operations run without approval.                                                                    |
| `maxRisk`                                                                                                            | *string*                                                                                                             | :heavy_minus_sign:                                                                                                   | Highest risk tier (`read-only`, `mutating`, `destructive`) a wildcard<br/>access request for these operations may cover. |