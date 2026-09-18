# DeploymentInputsResponseInput

Stack input definition serialized into a release stack.

## Example Usage

```typescript
import { DeploymentInputsResponseInput } from "@alienplatform/platform-api/models";

let value: DeploymentInputsResponseInput = {
  description: "amid lone highly down kiddingly rot",
  id: "<id>",
  kind: "enum",
  label: "<value>",
  providedBy: [
    "deployer",
  ],
  required: false,
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `default`                                                                                      | *models.DeploymentInputsResponseDefaultUnion*                                                  | :heavy_minus_sign:                                                                             | N/A                                                                                            |
| `description`                                                                                  | *string*                                                                                       | :heavy_check_mark:                                                                             | Human-facing helper text.                                                                      |
| `env`                                                                                          | [models.DeploymentInputsResponseEnv](../models/deploymentinputsresponseenv.md)[]               | :heavy_minus_sign:                                                                             | Runtime env-var mappings for v1 input resolution.                                              |
| `id`                                                                                           | *string*                                                                                       | :heavy_check_mark:                                                                             | Stable input ID used by CLI/API calls.                                                         |
| `kind`                                                                                         | [models.DeploymentInputsResponseKind](../models/deploymentinputsresponsekind.md)               | :heavy_check_mark:                                                                             | Primitive stack input kind.                                                                    |
| `label`                                                                                        | *string*                                                                                       | :heavy_check_mark:                                                                             | Human-facing field label.                                                                      |
| `placeholder`                                                                                  | *string*                                                                                       | :heavy_minus_sign:                                                                             | Example placeholder shown in UI.                                                               |
| `platforms`                                                                                    | [models.DeploymentInputsResponsePlatform](../models/deploymentinputsresponseplatform.md)[]     | :heavy_minus_sign:                                                                             | Platforms where this input applies.                                                            |
| `providedBy`                                                                                   | [models.DeploymentInputsResponseProvidedBy](../models/deploymentinputsresponseprovidedby.md)[] | :heavy_check_mark:                                                                             | Who can provide this value.                                                                    |
| `required`                                                                                     | *boolean*                                                                                      | :heavy_check_mark:                                                                             | Whether a resolved value is required before deployment can proceed.                            |
| `validation`                                                                                   | *models.DeploymentInputsResponseValidationUnion*                                               | :heavy_minus_sign:                                                                             | N/A                                                                                            |