# SyncReconcileResponseCurrentReleaseInput

Stack input definition serialized into a release stack.

## Example Usage

```typescript
import { SyncReconcileResponseCurrentReleaseInput } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseCurrentReleaseInput = {
  description:
    "forenenst deselect however knickers modulo yippee telescope provision fooey",
  id: "<id>",
  kind: "integer",
  label: "<value>",
  providedBy: [],
  required: true,
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `default`                                                                                                            | *models.SyncReconcileResponseCurrentReleaseDefaultUnion*                                                             | :heavy_minus_sign:                                                                                                   | N/A                                                                                                                  |
| `description`                                                                                                        | *string*                                                                                                             | :heavy_check_mark:                                                                                                   | Human-facing helper text.                                                                                            |
| `env`                                                                                                                | [models.SyncReconcileResponseCurrentReleaseEnv](../models/syncreconcileresponsecurrentreleaseenv.md)[]               | :heavy_minus_sign:                                                                                                   | Runtime env-var mappings for v1 input resolution.                                                                    |
| `id`                                                                                                                 | *string*                                                                                                             | :heavy_check_mark:                                                                                                   | Stable input ID used by CLI/API calls.                                                                               |
| `kind`                                                                                                               | [models.SyncReconcileResponseCurrentReleaseKind](../models/syncreconcileresponsecurrentreleasekind.md)               | :heavy_check_mark:                                                                                                   | Primitive stack input kind.                                                                                          |
| `label`                                                                                                              | *string*                                                                                                             | :heavy_check_mark:                                                                                                   | Human-facing field label.                                                                                            |
| `placeholder`                                                                                                        | *string*                                                                                                             | :heavy_minus_sign:                                                                                                   | Example placeholder shown in UI.                                                                                     |
| `platforms`                                                                                                          | [models.SyncReconcileResponseCurrentReleasePlatform](../models/syncreconcileresponsecurrentreleaseplatform.md)[]     | :heavy_minus_sign:                                                                                                   | Platforms where this input applies.                                                                                  |
| `providedBy`                                                                                                         | [models.SyncReconcileResponseCurrentReleaseProvidedBy](../models/syncreconcileresponsecurrentreleaseprovidedby.md)[] | :heavy_check_mark:                                                                                                   | Who can provide this value.                                                                                          |
| `required`                                                                                                           | *boolean*                                                                                                            | :heavy_check_mark:                                                                                                   | Whether a resolved value is required before deployment can proceed.                                                  |
| `validation`                                                                                                         | *models.SyncReconcileResponseCurrentReleaseValidationUnion*                                                          | :heavy_minus_sign:                                                                                                   | N/A                                                                                                                  |