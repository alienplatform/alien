# OperationApproval

A decision plus the highest risk tier a wildcard access request may cover.

## Example Usage

```typescript
import { OperationApproval } from "@alienplatform/manager-api/models";

let value: OperationApproval = {
  decision: "auto",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `decision`                                                                                                           | [models.OperationApprovalDecision](../models/operationapprovaldecision.md)                                           | :heavy_check_mark:                                                                                                   | Whether matching operations run without approval.                                                                    |
| `maxRisk`                                                                                                            | *string*                                                                                                             | :heavy_minus_sign:                                                                                                   | Highest risk tier (`read-only`, `mutating`, `destructive`) a wildcard<br/>access request for these operations may cover. |
