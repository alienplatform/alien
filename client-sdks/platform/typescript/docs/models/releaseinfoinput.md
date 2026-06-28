# ReleaseInfoInput

Stack input definition serialized into a release stack.

## Example Usage

```typescript
import { ReleaseInfoInput } from "@alienplatform/platform-api/models";

let value: ReleaseInfoInput = {
  description: "for aw paralyse if stable",
  id: "<id>",
  kind: "string",
  label: "<value>",
  providedBy: [],
  required: true,
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `default`                                                            | *models.ReleaseInfoDefaultUnion*                                     | :heavy_minus_sign:                                                   | N/A                                                                  |
| `description`                                                        | *string*                                                             | :heavy_check_mark:                                                   | Human-facing helper text.                                            |
| `env`                                                                | [models.ReleaseInfoEnv](../models/releaseinfoenv.md)[]               | :heavy_minus_sign:                                                   | Runtime env-var mappings for v1 input resolution.                    |
| `id`                                                                 | *string*                                                             | :heavy_check_mark:                                                   | Stable input ID used by CLI/API calls.                               |
| `kind`                                                               | [models.ReleaseInfoKind](../models/releaseinfokind.md)               | :heavy_check_mark:                                                   | Primitive stack input kind.                                          |
| `label`                                                              | *string*                                                             | :heavy_check_mark:                                                   | Human-facing field label.                                            |
| `placeholder`                                                        | *string*                                                             | :heavy_minus_sign:                                                   | Example placeholder shown in UI.                                     |
| `platforms`                                                          | [models.ReleaseInfoPlatform](../models/releaseinfoplatform.md)[]     | :heavy_minus_sign:                                                   | Platforms where this input applies.                                  |
| `providedBy`                                                         | [models.ReleaseInfoProvidedBy](../models/releaseinfoprovidedby.md)[] | :heavy_check_mark:                                                   | Who can provide this value.                                          |
| `required`                                                           | *boolean*                                                            | :heavy_check_mark:                                                   | Whether a resolved value is required before deployment can proceed.  |
| `validation`                                                         | *models.ReleaseInfoValidationUnion*                                  | :heavy_minus_sign:                                                   | N/A                                                                  |