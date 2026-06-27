# SyncAcquireResponseTargetReleaseInput

Stack input definition serialized into a release stack.

## Example Usage

```typescript
import { SyncAcquireResponseTargetReleaseInput } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseTargetReleaseInput = {
  description:
    "redact wearily disgorge possible gleefully metal gee terrorise proselytise amongst",
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
| `default`                                                                                                        | *models.SyncAcquireResponseTargetReleaseDefaultUnion*                                                            | :heavy_minus_sign:                                                                                               | N/A                                                                                                              |
| `description`                                                                                                    | *string*                                                                                                         | :heavy_check_mark:                                                                                               | Human-facing helper text.                                                                                        |
| `env`                                                                                                            | [models.SyncAcquireResponseTargetReleaseEnv](../models/syncacquireresponsetargetreleaseenv.md)[]                 | :heavy_minus_sign:                                                                                               | Runtime env-var mappings for v1 input resolution.                                                                |
| `id`                                                                                                             | *string*                                                                                                         | :heavy_check_mark:                                                                                               | Stable input ID used by CLI/API calls.                                                                           |
| `kind`                                                                                                           | [models.SyncAcquireResponseTargetReleaseKind](../models/syncacquireresponsetargetreleasekind.md)                 | :heavy_check_mark:                                                                                               | Primitive stack input kind.                                                                                      |
| `label`                                                                                                          | *string*                                                                                                         | :heavy_check_mark:                                                                                               | Human-facing field label.                                                                                        |
| `placeholder`                                                                                                    | *string*                                                                                                         | :heavy_minus_sign:                                                                                               | Example placeholder shown in UI.                                                                                 |
| `platforms`                                                                                                      | [models.SyncAcquireResponseTargetReleasePlatform](../models/syncacquireresponsetargetreleaseplatform.md)[]       | :heavy_minus_sign:                                                                                               | Platforms where this input applies.                                                                              |
| `providedBy`                                                                                                     | [models.SyncAcquireResponseTargetReleaseProvidedBy](../models/syncacquireresponsetargetreleaseprovidedby.md)[]   | :heavy_check_mark:                                                                                               | Who can provide this value.                                                                                      |
| `required`                                                                                                       | *boolean*                                                                                                        | :heavy_check_mark:                                                                                               | Whether a resolved value is required before deployment can proceed.                                              |
| `setupMethods`                                                                                                   | [models.SyncAcquireResponseTargetReleaseSetupMethod](../models/syncacquireresponsetargetreleasesetupmethod.md)[] | :heavy_minus_sign:                                                                                               | Setup methods where this input applies.                                                                          |
| `validation`                                                                                                     | *models.SyncAcquireResponseTargetReleaseValidationUnion*                                                         | :heavy_minus_sign:                                                                                               | N/A                                                                                                              |