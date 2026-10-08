# PersistImportedDeploymentRequestPendingPreparedStackApprovalPlugins

## Example Usage

```typescript
import { PersistImportedDeploymentRequestPendingPreparedStackApprovalPlugins } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestPendingPreparedStackApprovalPlugins =
  {
    decision: "auto",
  };
```

## Fields

| Field                                                                                                                                                          | Type                                                                                                                                                           | Required                                                                                                                                                       | Description                                                                                                                                                    |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `decision`                                                                                                                                                     | [models.PersistImportedDeploymentRequestPendingPreparedStackPluginsDecision](../models/persistimporteddeploymentrequestpendingpreparedstackpluginsdecision.md) | :heavy_check_mark:                                                                                                                                             | Whether matching operations run without approval.                                                                                                              |
| `maxRisk`                                                                                                                                                      | *string*                                                                                                                                                       | :heavy_minus_sign:                                                                                                                                             | Highest risk tier (`read-only`, `mutating`, `destructive`) a wildcard<br/>access request for these operations may cover.                                       |