# SyncReconcileResponseTargetReleaseInput

Stack input definition serialized into a release stack.

## Example Usage

```typescript
import { SyncReconcileResponseTargetReleaseInput } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseTargetReleaseInput = {
  description: "cornet impartial hmph some",
  id: "<id>",
  kind: "string",
  label: "<value>",
  providedBy: [],
  required: false,
};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `default`                                                                                                          | *models.SyncReconcileResponseTargetReleaseDefaultUnion*                                                            | :heavy_minus_sign:                                                                                                 | N/A                                                                                                                |
| `description`                                                                                                      | *string*                                                                                                           | :heavy_check_mark:                                                                                                 | Human-facing helper text.                                                                                          |
| `env`                                                                                                              | [models.SyncReconcileResponseTargetReleaseEnv](../models/syncreconcileresponsetargetreleaseenv.md)[]               | :heavy_minus_sign:                                                                                                 | Runtime env-var mappings for v1 input resolution.                                                                  |
| `id`                                                                                                               | *string*                                                                                                           | :heavy_check_mark:                                                                                                 | Stable input ID used by CLI/API calls.                                                                             |
| `kind`                                                                                                             | [models.SyncReconcileResponseTargetReleaseKind](../models/syncreconcileresponsetargetreleasekind.md)               | :heavy_check_mark:                                                                                                 | Primitive stack input kind.                                                                                        |
| `label`                                                                                                            | *string*                                                                                                           | :heavy_check_mark:                                                                                                 | Human-facing field label.                                                                                          |
| `placeholder`                                                                                                      | *string*                                                                                                           | :heavy_minus_sign:                                                                                                 | Example placeholder shown in UI.                                                                                   |
| `platforms`                                                                                                        | [models.SyncReconcileResponseTargetReleasePlatform](../models/syncreconcileresponsetargetreleaseplatform.md)[]     | :heavy_minus_sign:                                                                                                 | Platforms where this input applies.                                                                                |
| `providedBy`                                                                                                       | [models.SyncReconcileResponseTargetReleaseProvidedBy](../models/syncreconcileresponsetargetreleaseprovidedby.md)[] | :heavy_check_mark:                                                                                                 | Who can provide this value.                                                                                        |
| `required`                                                                                                         | *boolean*                                                                                                          | :heavy_check_mark:                                                                                                 | Whether a resolved value is required before deployment can proceed.                                                |
| `validation`                                                                                                       | *models.SyncReconcileResponseTargetReleaseValidationUnion*                                                         | :heavy_minus_sign:                                                                                                 | N/A                                                                                                                |