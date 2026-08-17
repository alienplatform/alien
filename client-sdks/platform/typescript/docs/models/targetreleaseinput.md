# TargetReleaseInput

Stack input definition serialized into a release stack.

## Example Usage

```typescript
import { TargetReleaseInput } from "@alienplatform/platform-api/models";

let value: TargetReleaseInput = {
  description: "junior because but well gah excitedly woot um bar",
  id: "<id>",
  kind: "number",
  label: "<value>",
  providedBy: [
    "developer",
  ],
  required: false,
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `default`                                                                | *models.TargetReleaseDefaultUnion*                                       | :heavy_minus_sign:                                                       | N/A                                                                      |
| `description`                                                            | *string*                                                                 | :heavy_check_mark:                                                       | Human-facing helper text.                                                |
| `env`                                                                    | [models.TargetReleaseEnv](../models/targetreleaseenv.md)[]               | :heavy_minus_sign:                                                       | Runtime env-var mappings for v1 input resolution.                        |
| `id`                                                                     | *string*                                                                 | :heavy_check_mark:                                                       | Stable input ID used by CLI/API calls.                                   |
| `kind`                                                                   | [models.TargetReleaseKind](../models/targetreleasekind.md)               | :heavy_check_mark:                                                       | Primitive stack input kind.                                              |
| `label`                                                                  | *string*                                                                 | :heavy_check_mark:                                                       | Human-facing field label.                                                |
| `placeholder`                                                            | *string*                                                                 | :heavy_minus_sign:                                                       | Example placeholder shown in UI.                                         |
| `platforms`                                                              | [models.TargetReleasePlatform](../models/targetreleaseplatform.md)[]     | :heavy_minus_sign:                                                       | Platforms where this input applies.                                      |
| `providedBy`                                                             | [models.TargetReleaseProvidedBy](../models/targetreleaseprovidedby.md)[] | :heavy_check_mark:                                                       | Who can provide this value.                                              |
| `required`                                                               | *boolean*                                                                | :heavy_check_mark:                                                       | Whether a resolved value is required before deployment can proceed.      |
| `validation`                                                             | *models.TargetReleaseValidationUnion*                                    | :heavy_minus_sign:                                                       | N/A                                                                      |