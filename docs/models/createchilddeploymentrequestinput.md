# CreateChildDeploymentRequestInput

Stack input definition serialized into a release stack.

## Example Usage

```typescript
import { CreateChildDeploymentRequestInput } from "@alienplatform/platform-api/models";

let value: CreateChildDeploymentRequestInput = {
  description: "hotfoot fledgling our",
  id: "<id>",
  kind: "number",
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
| `default`                                                                                              | *models.CreateChildDeploymentRequestDefaultUnion*                                                      | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `description`                                                                                          | *string*                                                                                               | :heavy_check_mark:                                                                                     | Human-facing helper text.                                                                              |
| `env`                                                                                                  | [models.CreateChildDeploymentRequestEnv](../models/createchilddeploymentrequestenv.md)[]               | :heavy_minus_sign:                                                                                     | Runtime env-var mappings for v1 input resolution.                                                      |
| `id`                                                                                                   | *string*                                                                                               | :heavy_check_mark:                                                                                     | Stable input ID used by CLI/API calls.                                                                 |
| `kind`                                                                                                 | [models.CreateChildDeploymentRequestKind](../models/createchilddeploymentrequestkind.md)               | :heavy_check_mark:                                                                                     | Primitive stack input kind.                                                                            |
| `label`                                                                                                | *string*                                                                                               | :heavy_check_mark:                                                                                     | Human-facing field label.                                                                              |
| `placeholder`                                                                                          | *string*                                                                                               | :heavy_minus_sign:                                                                                     | Example placeholder shown in UI.                                                                       |
| `platforms`                                                                                            | [models.CreateChildDeploymentRequestPlatform](../models/createchilddeploymentrequestplatform.md)[]     | :heavy_minus_sign:                                                                                     | Platforms where this input applies.                                                                    |
| `providedBy`                                                                                           | [models.CreateChildDeploymentRequestProvidedBy](../models/createchilddeploymentrequestprovidedby.md)[] | :heavy_check_mark:                                                                                     | Who can provide this value.                                                                            |
| `required`                                                                                             | *boolean*                                                                                              | :heavy_check_mark:                                                                                     | Whether a resolved value is required before deployment can proceed.                                    |
| `validation`                                                                                           | *models.CreateChildDeploymentRequestValidationUnion*                                                   | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |