# SyncAcquireResponseCurrentReleaseInput

Stack input definition serialized into a release stack.

## Example Usage

```typescript
import { SyncAcquireResponseCurrentReleaseInput } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseCurrentReleaseInput = {
  description: "longingly twine afore phew self-reliant supposing",
  id: "<id>",
  kind: "secret",
  label: "<value>",
  providedBy: [
    "deployer",
  ],
  required: false,
};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `default`                                                                                                          | *models.SyncAcquireResponseCurrentReleaseDefaultUnion*                                                             | :heavy_minus_sign:                                                                                                 | N/A                                                                                                                |
| `description`                                                                                                      | *string*                                                                                                           | :heavy_check_mark:                                                                                                 | Human-facing helper text.                                                                                          |
| `env`                                                                                                              | [models.SyncAcquireResponseCurrentReleaseEnv](../models/syncacquireresponsecurrentreleaseenv.md)[]                 | :heavy_minus_sign:                                                                                                 | Runtime env-var mappings for v1 input resolution.                                                                  |
| `id`                                                                                                               | *string*                                                                                                           | :heavy_check_mark:                                                                                                 | Stable input ID used by CLI/API calls.                                                                             |
| `kind`                                                                                                             | [models.SyncAcquireResponseCurrentReleaseKind](../models/syncacquireresponsecurrentreleasekind.md)                 | :heavy_check_mark:                                                                                                 | Primitive stack input kind.                                                                                        |
| `label`                                                                                                            | *string*                                                                                                           | :heavy_check_mark:                                                                                                 | Human-facing field label.                                                                                          |
| `placeholder`                                                                                                      | *string*                                                                                                           | :heavy_minus_sign:                                                                                                 | Example placeholder shown in UI.                                                                                   |
| `platforms`                                                                                                        | [models.SyncAcquireResponseCurrentReleasePlatform](../models/syncacquireresponsecurrentreleaseplatform.md)[]       | :heavy_minus_sign:                                                                                                 | Platforms where this input applies.                                                                                |
| `providedBy`                                                                                                       | [models.SyncAcquireResponseCurrentReleaseProvidedBy](../models/syncacquireresponsecurrentreleaseprovidedby.md)[]   | :heavy_check_mark:                                                                                                 | Who can provide this value.                                                                                        |
| `required`                                                                                                         | *boolean*                                                                                                          | :heavy_check_mark:                                                                                                 | Whether a resolved value is required before deployment can proceed.                                                |
| `setupMethods`                                                                                                     | [models.SyncAcquireResponseCurrentReleaseSetupMethod](../models/syncacquireresponsecurrentreleasesetupmethod.md)[] | :heavy_minus_sign:                                                                                                 | Setup methods where this input applies.                                                                            |
| `validation`                                                                                                       | *models.SyncAcquireResponseCurrentReleaseValidationUnion*                                                          | :heavy_minus_sign:                                                                                                 | N/A                                                                                                                |