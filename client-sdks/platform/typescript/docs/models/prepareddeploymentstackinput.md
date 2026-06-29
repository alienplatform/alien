# PreparedDeploymentStackInput

Stack input definition serialized into a release stack.

## Example Usage

```typescript
import { PreparedDeploymentStackInput } from "@alienplatform/platform-api/models";

let value: PreparedDeploymentStackInput = {
  description: "repossess blah farm",
  id: "<id>",
  kind: "string",
  label: "<value>",
  providedBy: [],
  required: false,
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `default`                                                                                          | *models.PreparedDeploymentStackDefaultUnion*                                                       | :heavy_minus_sign:                                                                                 | N/A                                                                                                |
| `description`                                                                                      | *string*                                                                                           | :heavy_check_mark:                                                                                 | Human-facing helper text.                                                                          |
| `env`                                                                                              | [models.PreparedDeploymentStackEnv](../models/prepareddeploymentstackenv.md)[]                     | :heavy_minus_sign:                                                                                 | Runtime env-var mappings for v1 input resolution.                                                  |
| `id`                                                                                               | *string*                                                                                           | :heavy_check_mark:                                                                                 | Stable input ID used by CLI/API calls.                                                             |
| `kind`                                                                                             | [models.PreparedDeploymentStackKind](../models/prepareddeploymentstackkind.md)                     | :heavy_check_mark:                                                                                 | Primitive stack input kind.                                                                        |
| `label`                                                                                            | *string*                                                                                           | :heavy_check_mark:                                                                                 | Human-facing field label.                                                                          |
| `placeholder`                                                                                      | *string*                                                                                           | :heavy_minus_sign:                                                                                 | Example placeholder shown in UI.                                                                   |
| `platforms`                                                                                        | [models.PreparedDeploymentStackStackPlatform](../models/prepareddeploymentstackstackplatform.md)[] | :heavy_minus_sign:                                                                                 | Platforms where this input applies.                                                                |
| `providedBy`                                                                                       | [models.PreparedDeploymentStackProvidedBy](../models/prepareddeploymentstackprovidedby.md)[]       | :heavy_check_mark:                                                                                 | Who can provide this value.                                                                        |
| `required`                                                                                         | *boolean*                                                                                          | :heavy_check_mark:                                                                                 | Whether a resolved value is required before deployment can proceed.                                |
| `validation`                                                                                       | *models.PreparedDeploymentStackValidationUnion*                                                    | :heavy_minus_sign:                                                                                 | N/A                                                                                                |