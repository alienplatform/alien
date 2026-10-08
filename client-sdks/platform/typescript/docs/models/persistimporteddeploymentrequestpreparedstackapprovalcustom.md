# PersistImportedDeploymentRequestPreparedStackApprovalCustom

## Example Usage

```typescript
import { PersistImportedDeploymentRequestPreparedStackApprovalCustom } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestPreparedStackApprovalCustom = {
  decision: "auto",
};
```

## Fields

| Field                                                                                                                                          | Type                                                                                                                                           | Required                                                                                                                                       | Description                                                                                                                                    |
| ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| `decision`                                                                                                                                     | [models.PersistImportedDeploymentRequestPreparedStackCustomDecision](../models/persistimporteddeploymentrequestpreparedstackcustomdecision.md) | :heavy_check_mark:                                                                                                                             | Whether matching operations run without approval.                                                                                              |
| `maxRisk`                                                                                                                                      | *string*                                                                                                                                       | :heavy_minus_sign:                                                                                                                             | Highest risk tier (`read-only`, `mutating`, `destructive`) a wildcard<br/>access request for these operations may cover.                       |