# SyncReconcileRequestCurrentReleaseInput

Stack input definition serialized into a release stack.

## Example Usage

```typescript
import { SyncReconcileRequestCurrentReleaseInput } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestCurrentReleaseInput = {
  description: "suspiciously eventually than ill and stay",
  id: "<id>",
  kind: "string",
  label: "<value>",
  providedBy: [
    "developer",
  ],
  required: true,
};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `default`                                                                                                          | *models.SyncReconcileRequestCurrentReleaseDefaultUnion*                                                            | :heavy_minus_sign:                                                                                                 | N/A                                                                                                                |
| `description`                                                                                                      | *string*                                                                                                           | :heavy_check_mark:                                                                                                 | Human-facing helper text.                                                                                          |
| `env`                                                                                                              | [models.SyncReconcileRequestCurrentReleaseEnv](../models/syncreconcilerequestcurrentreleaseenv.md)[]               | :heavy_minus_sign:                                                                                                 | Runtime env-var mappings for v1 input resolution.                                                                  |
| `id`                                                                                                               | *string*                                                                                                           | :heavy_check_mark:                                                                                                 | Stable input ID used by CLI/API calls.                                                                             |
| `kind`                                                                                                             | [models.CurrentReleaseStateKind](../models/currentreleasestatekind.md)                                             | :heavy_check_mark:                                                                                                 | Primitive stack input kind.                                                                                        |
| `label`                                                                                                            | *string*                                                                                                           | :heavy_check_mark:                                                                                                 | Human-facing field label.                                                                                          |
| `placeholder`                                                                                                      | *string*                                                                                                           | :heavy_minus_sign:                                                                                                 | Example placeholder shown in UI.                                                                                   |
| `platforms`                                                                                                        | [models.SyncReconcileRequestCurrentReleasePlatform](../models/syncreconcilerequestcurrentreleaseplatform.md)[]     | :heavy_minus_sign:                                                                                                 | Platforms where this input applies.                                                                                |
| `providedBy`                                                                                                       | [models.SyncReconcileRequestCurrentReleaseProvidedBy](../models/syncreconcilerequestcurrentreleaseprovidedby.md)[] | :heavy_check_mark:                                                                                                 | Who can provide this value.                                                                                        |
| `required`                                                                                                         | *boolean*                                                                                                          | :heavy_check_mark:                                                                                                 | Whether a resolved value is required before deployment can proceed.                                                |
| `validation`                                                                                                       | *models.SyncReconcileRequestCurrentReleaseValidationUnion*                                                         | :heavy_minus_sign:                                                                                                 | N/A                                                                                                                |