# PersistImportedDeploymentRequestLocation

Where a deployer writes a vault-native secret.

## Example Usage

```typescript
import { PersistImportedDeploymentRequestLocation } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestLocation = {
  cliCommand: "<value>",
  name: "<value>",
  store: "gcp-secret-manager",
};
```

## Fields

| Field                                                                                                         | Type                                                                                                          | Required                                                                                                      | Description                                                                                                   |
| ------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| `cliCommand`                                                                                                  | *string*                                                                                                      | :heavy_check_mark:                                                                                            | Command that writes the value, with `<VALUE>` in place of the secret.                                         |
| `consoleUrl`                                                                                                  | *string*                                                                                                      | :heavy_minus_sign:                                                                                            | Cloud console page where the secret is created, when the store has one.                                       |
| `name`                                                                                                        | *string*                                                                                                      | :heavy_check_mark:                                                                                            | Full name of the secret in that store.                                                                        |
| `store`                                                                                                       | [models.PersistImportedDeploymentRequestStore](../models/persistimporteddeploymentrequeststore.md)            | :heavy_check_mark:                                                                                            | The secret store a deployer writes a vault-native secret into.                                                |
| `vaultName`                                                                                                   | *string*                                                                                                      | :heavy_minus_sign:                                                                                            | The Azure Key Vault that holds the secret. Other stores resolve `name`<br/>in the stack's own account or project. |