# TargetDeploymentInput

Stack input definition serialized into a release stack.

## Example Usage

```typescript
import { TargetDeploymentInput } from "@alienplatform/platform-api/models";

let value: TargetDeploymentInput = {
  description:
    "despite joyfully within coal bah mushy against voluntarily although",
  id: "<id>",
  kind: "enum",
  label: "<value>",
  providedBy: [],
  required: false,
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `default`                                                                      | *models.TargetDeploymentDefaultUnion*                                          | :heavy_minus_sign:                                                             | N/A                                                                            |
| `description`                                                                  | *string*                                                                       | :heavy_check_mark:                                                             | Human-facing helper text.                                                      |
| `env`                                                                          | [models.TargetDeploymentEnv](../models/targetdeploymentenv.md)[]               | :heavy_minus_sign:                                                             | Runtime env-var mappings for v1 input resolution.                              |
| `id`                                                                           | *string*                                                                       | :heavy_check_mark:                                                             | Stable input ID used by CLI/API calls.                                         |
| `kind`                                                                         | [models.TargetDeploymentKind](../models/targetdeploymentkind.md)               | :heavy_check_mark:                                                             | Primitive stack input kind.                                                    |
| `label`                                                                        | *string*                                                                       | :heavy_check_mark:                                                             | Human-facing field label.                                                      |
| `placeholder`                                                                  | *string*                                                                       | :heavy_minus_sign:                                                             | Example placeholder shown in UI.                                               |
| `platforms`                                                                    | [models.ReleaseInfoPlatform](../models/releaseinfoplatform.md)[]               | :heavy_minus_sign:                                                             | Platforms where this input applies.                                            |
| `providedBy`                                                                   | [models.TargetDeploymentProvidedBy](../models/targetdeploymentprovidedby.md)[] | :heavy_check_mark:                                                             | Who can provide this value.                                                    |
| `required`                                                                     | *boolean*                                                                      | :heavy_check_mark:                                                             | Whether a resolved value is required before deployment can proceed.            |
| `validation`                                                                   | *models.TargetDeploymentValidationUnion*                                       | :heavy_minus_sign:                                                             | N/A                                                                            |