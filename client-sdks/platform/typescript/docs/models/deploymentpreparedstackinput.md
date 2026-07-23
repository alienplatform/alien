# DeploymentPreparedStackInput

Stack input definition serialized into a release stack.

## Example Usage

```typescript
import { DeploymentPreparedStackInput } from "@alienplatform/platform-api/models";

let value: DeploymentPreparedStackInput = {
  description: "formation um oh",
  id: "<id>",
  kind: "number",
  label: "<value>",
  providedBy: [],
  required: true,
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `default`                                                                                    | *models.DeploymentPreparedStackDefaultUnion*                                                 | :heavy_minus_sign:                                                                           | N/A                                                                                          |
| `description`                                                                                | *string*                                                                                     | :heavy_check_mark:                                                                           | Human-facing helper text.                                                                    |
| `env`                                                                                        | [models.DeploymentPreparedStackEnv](../models/deploymentpreparedstackenv.md)[]               | :heavy_minus_sign:                                                                           | Runtime env-var mappings for v1 input resolution.                                            |
| `id`                                                                                         | *string*                                                                                     | :heavy_check_mark:                                                                           | Stable input ID used by CLI/API calls.                                                       |
| `kind`                                                                                       | [models.DeploymentPreparedStackKind](../models/deploymentpreparedstackkind.md)               | :heavy_check_mark:                                                                           | Primitive stack input kind.                                                                  |
| `label`                                                                                      | *string*                                                                                     | :heavy_check_mark:                                                                           | Human-facing field label.                                                                    |
| `placeholder`                                                                                | *string*                                                                                     | :heavy_minus_sign:                                                                           | Example placeholder shown in UI.                                                             |
| `platforms`                                                                                  | [models.DeploymentPreparedStackPlatform](../models/deploymentpreparedstackplatform.md)[]     | :heavy_minus_sign:                                                                           | Platforms where this input applies.                                                          |
| `providedBy`                                                                                 | [models.DeploymentPreparedStackProvidedBy](../models/deploymentpreparedstackprovidedby.md)[] | :heavy_check_mark:                                                                           | Who can provide this value.                                                                  |
| `required`                                                                                   | *boolean*                                                                                    | :heavy_check_mark:                                                                           | Whether a resolved value is required before deployment can proceed.                          |
| `validation`                                                                                 | *models.DeploymentPreparedStackValidationUnion*                                              | :heavy_minus_sign:                                                                           | N/A                                                                                          |
