# SyncReconcileResponsePreparedStackInput

Stack input definition serialized into a release stack.

## Example Usage

```typescript
import { SyncReconcileResponsePreparedStackInput } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponsePreparedStackInput = {
  description: "minister youthfully than",
  id: "<id>",
  kind: "secret",
  label: "<value>",
  providedBy: [],
  required: false,
};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `default`                                                                                                          | *models.SyncReconcileResponsePreparedStackDefaultUnion*                                                            | :heavy_minus_sign:                                                                                                 | N/A                                                                                                                |
| `description`                                                                                                      | *string*                                                                                                           | :heavy_check_mark:                                                                                                 | Human-facing helper text.                                                                                          |
| `env`                                                                                                              | [models.SyncReconcileResponsePreparedStackEnv](../models/syncreconcileresponsepreparedstackenv.md)[]               | :heavy_minus_sign:                                                                                                 | Runtime env-var mappings for v1 input resolution.                                                                  |
| `id`                                                                                                               | *string*                                                                                                           | :heavy_check_mark:                                                                                                 | Stable input ID used by CLI/API calls.                                                                             |
| `kind`                                                                                                             | [models.SyncReconcileResponsePreparedStackKind](../models/syncreconcileresponsepreparedstackkind.md)               | :heavy_check_mark:                                                                                                 | Primitive stack input kind.                                                                                        |
| `label`                                                                                                            | *string*                                                                                                           | :heavy_check_mark:                                                                                                 | Human-facing field label.                                                                                          |
| `placeholder`                                                                                                      | *string*                                                                                                           | :heavy_minus_sign:                                                                                                 | Example placeholder shown in UI.                                                                                   |
| `platforms`                                                                                                        | [models.SyncReconcileResponsePreparedStackPlatform](../models/syncreconcileresponsepreparedstackplatform.md)[]     | :heavy_minus_sign:                                                                                                 | Platforms where this input applies.                                                                                |
| `providedBy`                                                                                                       | [models.SyncReconcileResponsePreparedStackProvidedBy](../models/syncreconcileresponsepreparedstackprovidedby.md)[] | :heavy_check_mark:                                                                                                 | Who can provide this value.                                                                                        |
| `required`                                                                                                         | *boolean*                                                                                                          | :heavy_check_mark:                                                                                                 | Whether a resolved value is required before deployment can proceed.                                                |
| `validation`                                                                                                       | *models.SyncReconcileResponsePreparedStackValidationUnion*                                                         | :heavy_minus_sign:                                                                                                 | N/A                                                                                                                |