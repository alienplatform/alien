# DeploymentInput

Stack input definition serialized into a release stack.

## Example Usage

```typescript
import { DeploymentInput } from "@alienplatform/platform-api/models";

let value: DeploymentInput = {
  description: "reasoning nice once left sit round aw",
  id: "<id>",
  kind: "string",
  label: "<value>",
  providedBy: [],
  required: true,
};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `default`                                                                                | *models.DeploymentDefaultUnion*                                                          | :heavy_minus_sign:                                                                       | N/A                                                                                      |
| `description`                                                                            | *string*                                                                                 | :heavy_check_mark:                                                                       | Human-facing helper text.                                                                |
| `env`                                                                                    | [models.DeploymentEnv](../models/deploymentenv.md)[]                                     | :heavy_minus_sign:                                                                       | Runtime env-var mappings for v1 input resolution.                                        |
| `id`                                                                                     | *string*                                                                                 | :heavy_check_mark:                                                                       | Stable input ID used by CLI/API calls.                                                   |
| `kind`                                                                                   | [models.DeploymentKind](../models/deploymentkind.md)                                     | :heavy_check_mark:                                                                       | Primitive stack input kind.                                                              |
| `label`                                                                                  | *string*                                                                                 | :heavy_check_mark:                                                                       | Human-facing field label.                                                                |
| `placeholder`                                                                            | *string*                                                                                 | :heavy_minus_sign:                                                                       | Example placeholder shown in UI.                                                         |
| `platforms`                                                                              | [models.DeploymentPreparedStackPlatform](../models/deploymentpreparedstackplatform.md)[] | :heavy_minus_sign:                                                                       | Platforms where this input applies.                                                      |
| `providedBy`                                                                             | [models.DeploymentProvidedBy](../models/deploymentprovidedby.md)[]                       | :heavy_check_mark:                                                                       | Who can provide this value.                                                              |
| `required`                                                                               | *boolean*                                                                                | :heavy_check_mark:                                                                       | Whether a resolved value is required before deployment can proceed.                      |
| `validation`                                                                             | *models.DeploymentValidationUnion*                                                       | :heavy_minus_sign:                                                                       | N/A                                                                                      |