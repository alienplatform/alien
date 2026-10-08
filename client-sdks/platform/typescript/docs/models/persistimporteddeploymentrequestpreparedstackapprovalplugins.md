# PersistImportedDeploymentRequestPreparedStackApprovalPlugins

## Example Usage

```typescript
import { PersistImportedDeploymentRequestPreparedStackApprovalPlugins } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestPreparedStackApprovalPlugins = {
  decision: "auto",
};
```

## Fields

| Field                                                                                                                                            | Type                                                                                                                                             | Required                                                                                                                                         | Description                                                                                                                                      |
| ------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| `decision`                                                                                                                                       | [models.PersistImportedDeploymentRequestPreparedStackPluginsDecision](../models/persistimporteddeploymentrequestpreparedstackpluginsdecision.md) | :heavy_check_mark:                                                                                                                               | Whether matching operations run without approval.                                                                                                |
| `maxRisk`                                                                                                                                        | *string*                                                                                                                                         | :heavy_minus_sign:                                                                                                                               | Highest risk tier (`read-only`, `mutating`, `destructive`) a wildcard<br/>access request for these operations may cover.                         |