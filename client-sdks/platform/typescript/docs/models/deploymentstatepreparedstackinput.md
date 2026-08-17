# DeploymentStatePreparedStackInput

Stack input definition serialized into a release stack.

## Example Usage

```typescript
import { DeploymentStatePreparedStackInput } from "@alienplatform/platform-api/models";

let value: DeploymentStatePreparedStackInput = {
  description: "ick dusk recount deck well late cow",
  id: "<id>",
  kind: "boolean",
  label: "<value>",
  providedBy: [
    "deployer",
  ],
  required: false,
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `default`                                                                                              | *models.DeploymentStatePreparedStackDefaultUnion*                                                      | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `description`                                                                                          | *string*                                                                                               | :heavy_check_mark:                                                                                     | Human-facing helper text.                                                                              |
| `env`                                                                                                  | [models.DeploymentStatePreparedStackEnv](../models/deploymentstatepreparedstackenv.md)[]               | :heavy_minus_sign:                                                                                     | Runtime env-var mappings for v1 input resolution.                                                      |
| `id`                                                                                                   | *string*                                                                                               | :heavy_check_mark:                                                                                     | Stable input ID used by CLI/API calls.                                                                 |
| `kind`                                                                                                 | [models.DeploymentStatePreparedStackKind](../models/deploymentstatepreparedstackkind.md)               | :heavy_check_mark:                                                                                     | Primitive stack input kind.                                                                            |
| `label`                                                                                                | *string*                                                                                               | :heavy_check_mark:                                                                                     | Human-facing field label.                                                                              |
| `placeholder`                                                                                          | *string*                                                                                               | :heavy_minus_sign:                                                                                     | Example placeholder shown in UI.                                                                       |
| `platforms`                                                                                            | [models.DeploymentStatePreparedStackPlatform](../models/deploymentstatepreparedstackplatform.md)[]     | :heavy_minus_sign:                                                                                     | Platforms where this input applies.                                                                    |
| `providedBy`                                                                                           | [models.DeploymentStatePreparedStackProvidedBy](../models/deploymentstatepreparedstackprovidedby.md)[] | :heavy_check_mark:                                                                                     | Who can provide this value.                                                                            |
| `required`                                                                                             | *boolean*                                                                                              | :heavy_check_mark:                                                                                     | Whether a resolved value is required before deployment can proceed.                                    |
| `validation`                                                                                           | *models.DeploymentStatePreparedStackValidationUnion*                                                   | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |