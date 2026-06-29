# SyncReconcileRequestTargetReleaseInput

Stack input definition serialized into a release stack.

## Example Usage

```typescript
import { SyncReconcileRequestTargetReleaseInput } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestTargetReleaseInput = {
  description: "without boo at gah casket",
  id: "<id>",
  kind: "stringList",
  label: "<value>",
  providedBy: [],
  required: true,
};
```

## Fields

| Field                                                                                                            | Type                                                                                                             | Required                                                                                                         | Description                                                                                                      |
| ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `default`                                                                                                        | *models.SyncReconcileRequestTargetReleaseDefaultUnion*                                                           | :heavy_minus_sign:                                                                                               | N/A                                                                                                              |
| `description`                                                                                                    | *string*                                                                                                         | :heavy_check_mark:                                                                                               | Human-facing helper text.                                                                                        |
| `env`                                                                                                            | [models.SyncReconcileRequestTargetReleaseEnv](../models/syncreconcilerequesttargetreleaseenv.md)[]               | :heavy_minus_sign:                                                                                               | Runtime env-var mappings for v1 input resolution.                                                                |
| `id`                                                                                                             | *string*                                                                                                         | :heavy_check_mark:                                                                                               | Stable input ID used by CLI/API calls.                                                                           |
| `kind`                                                                                                           | [models.TargetReleaseStateKind](../models/targetreleasestatekind.md)                                             | :heavy_check_mark:                                                                                               | Primitive stack input kind.                                                                                      |
| `label`                                                                                                          | *string*                                                                                                         | :heavy_check_mark:                                                                                               | Human-facing field label.                                                                                        |
| `placeholder`                                                                                                    | *string*                                                                                                         | :heavy_minus_sign:                                                                                               | Example placeholder shown in UI.                                                                                 |
| `platforms`                                                                                                      | [models.SyncReconcileRequestTargetReleasePlatform](../models/syncreconcilerequesttargetreleaseplatform.md)[]     | :heavy_minus_sign:                                                                                               | Platforms where this input applies.                                                                              |
| `providedBy`                                                                                                     | [models.SyncReconcileRequestTargetReleaseProvidedBy](../models/syncreconcilerequesttargetreleaseprovidedby.md)[] | :heavy_check_mark:                                                                                               | Who can provide this value.                                                                                      |
| `required`                                                                                                       | *boolean*                                                                                                        | :heavy_check_mark:                                                                                               | Whether a resolved value is required before deployment can proceed.                                              |
| `validation`                                                                                                     | *models.SyncReconcileRequestTargetReleaseValidationUnion*                                                        | :heavy_minus_sign:                                                                                               | N/A                                                                                                              |