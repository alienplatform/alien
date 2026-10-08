# PersistImportedDeploymentRequestPendingPreparedStackApprovalCustom

## Example Usage

```typescript
import { PersistImportedDeploymentRequestPendingPreparedStackApprovalCustom } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestPendingPreparedStackApprovalCustom =
  {
    decision: "manual",
  };
```

## Fields

| Field                                                                                                                                                        | Type                                                                                                                                                         | Required                                                                                                                                                     | Description                                                                                                                                                  |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `decision`                                                                                                                                                   | [models.PersistImportedDeploymentRequestPendingPreparedStackCustomDecision](../models/persistimporteddeploymentrequestpendingpreparedstackcustomdecision.md) | :heavy_check_mark:                                                                                                                                           | Whether matching operations run without approval.                                                                                                            |
| `maxRisk`                                                                                                                                                    | *string*                                                                                                                                                     | :heavy_minus_sign:                                                                                                                                           | Highest risk tier (`read-only`, `mutating`, `destructive`) a wildcard<br/>access request for these operations may cover.                                     |