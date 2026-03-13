# SyncReconcileResponseVariable

Environment variable for deployment

## Example Usage

```typescript
import { SyncReconcileResponseVariable } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponseVariable = {
  name: "<value>",
  type: "secret",
  value: "<value>",
};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `name`                                                                                                             | *string*                                                                                                           | :heavy_check_mark:                                                                                                 | Variable name                                                                                                      |
| `targetResources`                                                                                                  | *string*[]                                                                                                         | :heavy_minus_sign:                                                                                                 | Target resource patterns (null = all resources, Some = wildcard patterns)                                          |
| `type`                                                                                                             | [models.SyncReconcileResponseEnvironmentVariablesType](../models/syncreconcileresponseenvironmentvariablestype.md) | :heavy_check_mark:                                                                                                 | Type of environment variable                                                                                       |
| `value`                                                                                                            | *string*                                                                                                           | :heavy_check_mark:                                                                                                 | Variable value (decrypted - deployment has access to decryption keys)                                              |