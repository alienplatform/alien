# DeploymentPendingPreparedStackInput

Stack input definition serialized into a release stack.

## Example Usage

```typescript
import { DeploymentPendingPreparedStackInput } from "@alienplatform/platform-api/models";

let value: DeploymentPendingPreparedStackInput = {
  description: "smog concerning quash qua phooey wherever",
  id: "<id>",
  kind: "boolean",
  label: "<value>",
  providedBy: [],
  required: false,
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `default`                                                                                                  | *models.DeploymentPendingPreparedStackDefaultUnion*                                                        | :heavy_minus_sign:                                                                                         | N/A                                                                                                        |
| `description`                                                                                              | *string*                                                                                                   | :heavy_check_mark:                                                                                         | Human-facing helper text.                                                                                  |
| `env`                                                                                                      | [models.DeploymentPendingPreparedStackEnv](../models/deploymentpendingpreparedstackenv.md)[]               | :heavy_minus_sign:                                                                                         | Runtime env-var mappings for v1 input resolution.                                                          |
| `id`                                                                                                       | *string*                                                                                                   | :heavy_check_mark:                                                                                         | Stable input ID used by CLI/API calls.                                                                     |
| `kind`                                                                                                     | [models.DeploymentPendingPreparedStackKind](../models/deploymentpendingpreparedstackkind.md)               | :heavy_check_mark:                                                                                         | Primitive stack input kind.                                                                                |
| `label`                                                                                                    | *string*                                                                                                   | :heavy_check_mark:                                                                                         | Human-facing field label.                                                                                  |
| `placeholder`                                                                                              | *string*                                                                                                   | :heavy_minus_sign:                                                                                         | Example placeholder shown in UI.                                                                           |
| `platforms`                                                                                                | [models.DeploymentPendingPreparedStackPlatform](../models/deploymentpendingpreparedstackplatform.md)[]     | :heavy_minus_sign:                                                                                         | Platforms where this input applies.                                                                        |
| `providedBy`                                                                                               | [models.DeploymentPendingPreparedStackProvidedBy](../models/deploymentpendingpreparedstackprovidedby.md)[] | :heavy_check_mark:                                                                                         | Who can provide this value.                                                                                |
| `required`                                                                                                 | *boolean*                                                                                                  | :heavy_check_mark:                                                                                         | Whether a resolved value is required before deployment can proceed.                                        |
| `validation`                                                                                               | *models.DeploymentPendingPreparedStackValidationUnion*                                                     | :heavy_minus_sign:                                                                                         | N/A                                                                                                        |
