# SyncListResponsePendingPreparedStackApprovalPlugins

## Example Usage

```typescript
import { SyncListResponsePendingPreparedStackApprovalPlugins } from "@alienplatform/platform-api/models";

let value: SyncListResponsePendingPreparedStackApprovalPlugins = {
  decision: "manual",
};
```

## Fields

| Field                                                                                                                          | Type                                                                                                                           | Required                                                                                                                       | Description                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `decision`                                                                                                                     | [models.SyncListResponsePendingPreparedStackPluginsDecision](../models/synclistresponsependingpreparedstackpluginsdecision.md) | :heavy_check_mark:                                                                                                             | Whether matching operations run without approval.                                                                              |
| `maxRisk`                                                                                                                      | *string*                                                                                                                       | :heavy_minus_sign:                                                                                                             | Highest risk tier (`read-only`, `mutating`, `destructive`) a wildcard<br/>access request for these operations may cover.       |