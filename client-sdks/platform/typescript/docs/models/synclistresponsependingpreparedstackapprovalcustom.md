# SyncListResponsePendingPreparedStackApprovalCustom

## Example Usage

```typescript
import { SyncListResponsePendingPreparedStackApprovalCustom } from "@alienplatform/platform-api/models";

let value: SyncListResponsePendingPreparedStackApprovalCustom = {
  decision: "auto",
};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `decision`                                                                                                                   | [models.SyncListResponsePendingPreparedStackCustomDecision](../models/synclistresponsependingpreparedstackcustomdecision.md) | :heavy_check_mark:                                                                                                           | Whether matching operations run without approval.                                                                            |
| `maxRisk`                                                                                                                    | *string*                                                                                                                     | :heavy_minus_sign:                                                                                                           | Highest risk tier (`read-only`, `mutating`, `destructive`) a wildcard<br/>access request for these operations may cover.     |