# UpdateDeploymentInputsResponseInput

Stack input definition serialized into a release stack.

## Example Usage

```typescript
import { UpdateDeploymentInputsResponseInput } from "@alienplatform/platform-api/models";

let value: UpdateDeploymentInputsResponseInput = {
  description: "not delirious aw sorrowful abaft out recompense",
  id: "<id>",
  kind: "boolean",
  label: "<value>",
  providedBy: [],
  required: true,
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `default`                                                                                                  | *models.UpdateDeploymentInputsResponseDefaultUnion*                                                        | :heavy_minus_sign:                                                                                         | N/A                                                                                                        |
| `description`                                                                                              | *string*                                                                                                   | :heavy_check_mark:                                                                                         | Human-facing helper text.                                                                                  |
| `env`                                                                                                      | [models.UpdateDeploymentInputsResponseEnv](../models/updatedeploymentinputsresponseenv.md)[]               | :heavy_minus_sign:                                                                                         | Runtime env-var mappings for v1 input resolution.                                                          |
| `id`                                                                                                       | *string*                                                                                                   | :heavy_check_mark:                                                                                         | Stable input ID used by CLI/API calls.                                                                     |
| `kind`                                                                                                     | [models.UpdateDeploymentInputsResponseKind](../models/updatedeploymentinputsresponsekind.md)               | :heavy_check_mark:                                                                                         | Primitive stack input kind.                                                                                |
| `label`                                                                                                    | *string*                                                                                                   | :heavy_check_mark:                                                                                         | Human-facing field label.                                                                                  |
| `placeholder`                                                                                              | *string*                                                                                                   | :heavy_minus_sign:                                                                                         | Example placeholder shown in UI.                                                                           |
| `platforms`                                                                                                | [models.UpdateDeploymentInputsResponsePlatform](../models/updatedeploymentinputsresponseplatform.md)[]     | :heavy_minus_sign:                                                                                         | Platforms where this input applies.                                                                        |
| `providedBy`                                                                                               | [models.UpdateDeploymentInputsResponseProvidedBy](../models/updatedeploymentinputsresponseprovidedby.md)[] | :heavy_check_mark:                                                                                         | Who can provide this value.                                                                                |
| `required`                                                                                                 | *boolean*                                                                                                  | :heavy_check_mark:                                                                                         | Whether a resolved value is required before deployment can proceed.                                        |
| `validation`                                                                                               | *models.UpdateDeploymentInputsResponseValidationUnion*                                                     | :heavy_minus_sign:                                                                                         | N/A                                                                                                        |