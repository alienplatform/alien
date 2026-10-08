# SyncListResponseLocation

Where a deployer writes a vault-native secret.

## Example Usage

```typescript
import { SyncListResponseLocation } from "@alienplatform/platform-api/models";

let value: SyncListResponseLocation = {
  cliCommand: "<value>",
  name: "<value>",
  store: "aws-parameter-store",
};
```

## Fields

| Field                                                                                                                              | Type                                                                                                                               | Required                                                                                                                           | Description                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `cliCommand`                                                                                                                       | *string*                                                                                                                           | :heavy_check_mark:                                                                                                                 | Command that writes the value, with `<VALUE>` in place of the secret.                                                              |
| `consoleUrl`                                                                                                                       | *string*                                                                                                                           | :heavy_minus_sign:                                                                                                                 | Cloud console page where the secret is created, when the store has one.                                                            |
| `deleteCommand`                                                                                                                    | *string*                                                                                                                           | :heavy_minus_sign:                                                                                                                 | Command that deletes the secret. Deleting a deployment keeps the<br/>secrets the deployer wrote, since Alien never owned their values. |
| `name`                                                                                                                             | *string*                                                                                                                           | :heavy_check_mark:                                                                                                                 | Full name of the secret in that store.                                                                                             |
| `store`                                                                                                                            | [models.SyncListResponseStore](../models/synclistresponsestore.md)                                                                 | :heavy_check_mark:                                                                                                                 | The secret store a deployer writes a vault-native secret into.                                                                     |
| `vaultName`                                                                                                                        | *string*                                                                                                                           | :heavy_minus_sign:                                                                                                                 | The Azure Key Vault that holds the secret. Other stores resolve `name`<br/>in the stack's own account or project.                  |