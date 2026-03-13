# SyncReconcileResponseExternalBindingsParameterStore

AWS SSM Parameter Store vault binding configuration

## Example Usage

```typescript
import { SyncReconcileResponseExternalBindingsParameterStore } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponseExternalBindingsParameterStore = {
  service: "parameter-store",
  type: "vault",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `vaultPrefix`                                                                                                        | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"parameter-store"*                                                                                                  | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncReconcileResponseTypeVault1](../models/syncreconcileresponsetypevault1.md)                               | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |