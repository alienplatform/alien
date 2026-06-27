# SyncReconcileRequestPreparedStackInput

Stack input definition serialized into a release stack.

## Example Usage

```typescript
import { SyncReconcileRequestPreparedStackInput } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestPreparedStackInput = {
  description: "before ugh before polished considering",
  id: "<id>",
  kind: "enum",
  label: "<value>",
  providedBy: [
    "deployer",
  ],
  required: true,
};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `default`                                                                                                          | *models.SyncReconcileRequestPreparedStackDefaultUnion*                                                             | :heavy_minus_sign:                                                                                                 | N/A                                                                                                                |
| `description`                                                                                                      | *string*                                                                                                           | :heavy_check_mark:                                                                                                 | Human-facing helper text.                                                                                          |
| `env`                                                                                                              | [models.SyncReconcileRequestPreparedStackEnv](../models/syncreconcilerequestpreparedstackenv.md)[]                 | :heavy_minus_sign:                                                                                                 | Runtime env-var mappings for v1 input resolution.                                                                  |
| `id`                                                                                                               | *string*                                                                                                           | :heavy_check_mark:                                                                                                 | Stable input ID used by CLI/API calls.                                                                             |
| `kind`                                                                                                             | [models.PreparedStackStateKind](../models/preparedstackstatekind.md)                                               | :heavy_check_mark:                                                                                                 | Primitive stack input kind.                                                                                        |
| `label`                                                                                                            | *string*                                                                                                           | :heavy_check_mark:                                                                                                 | Human-facing field label.                                                                                          |
| `placeholder`                                                                                                      | *string*                                                                                                           | :heavy_minus_sign:                                                                                                 | Example placeholder shown in UI.                                                                                   |
| `platforms`                                                                                                        | [models.SyncReconcileRequestPreparedStackPlatform](../models/syncreconcilerequestpreparedstackplatform.md)[]       | :heavy_minus_sign:                                                                                                 | Platforms where this input applies.                                                                                |
| `providedBy`                                                                                                       | [models.SyncReconcileRequestPreparedStackProvidedBy](../models/syncreconcilerequestpreparedstackprovidedby.md)[]   | :heavy_check_mark:                                                                                                 | Who can provide this value.                                                                                        |
| `required`                                                                                                         | *boolean*                                                                                                          | :heavy_check_mark:                                                                                                 | Whether a resolved value is required before deployment can proceed.                                                |
| `setupMethods`                                                                                                     | [models.SyncReconcileRequestPreparedStackSetupMethod](../models/syncreconcilerequestpreparedstacksetupmethod.md)[] | :heavy_minus_sign:                                                                                                 | Setup methods where this input applies.                                                                            |
| `validation`                                                                                                       | *models.SyncReconcileRequestPreparedStackValidationUnion*                                                          | :heavy_minus_sign:                                                                                                 | N/A                                                                                                                |