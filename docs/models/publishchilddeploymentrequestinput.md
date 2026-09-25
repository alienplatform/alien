# PublishChildDeploymentRequestInput

Stack input definition serialized into a release stack.

## Example Usage

```typescript
import { PublishChildDeploymentRequestInput } from "@alienplatform/platform-api/models";

let value: PublishChildDeploymentRequestInput = {
  description:
    "towards pleasing gosh brr uh-huh cruelly sympathetically dependency with",
  id: "<id>",
  kind: "boolean",
  label: "<value>",
  providedBy: [],
  required: false,
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `default`                                                                                                | *models.PublishChildDeploymentRequestDefaultUnion*                                                       | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `description`                                                                                            | *string*                                                                                                 | :heavy_check_mark:                                                                                       | Human-facing helper text.                                                                                |
| `env`                                                                                                    | [models.PublishChildDeploymentRequestEnv](../models/publishchilddeploymentrequestenv.md)[]               | :heavy_minus_sign:                                                                                       | Runtime env-var mappings for v1 input resolution.                                                        |
| `id`                                                                                                     | *string*                                                                                                 | :heavy_check_mark:                                                                                       | Stable input ID used by CLI/API calls.                                                                   |
| `kind`                                                                                                   | [models.PublishChildDeploymentRequestKind](../models/publishchilddeploymentrequestkind.md)               | :heavy_check_mark:                                                                                       | Primitive stack input kind.                                                                              |
| `label`                                                                                                  | *string*                                                                                                 | :heavy_check_mark:                                                                                       | Human-facing field label.                                                                                |
| `placeholder`                                                                                            | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | Example placeholder shown in UI.                                                                         |
| `platforms`                                                                                              | [models.PublishChildDeploymentRequestPlatform](../models/publishchilddeploymentrequestplatform.md)[]     | :heavy_minus_sign:                                                                                       | Platforms where this input applies.                                                                      |
| `providedBy`                                                                                             | [models.PublishChildDeploymentRequestProvidedBy](../models/publishchilddeploymentrequestprovidedby.md)[] | :heavy_check_mark:                                                                                       | Who can provide this value.                                                                              |
| `required`                                                                                               | *boolean*                                                                                                | :heavy_check_mark:                                                                                       | Whether a resolved value is required before deployment can proceed.                                      |
| `validation`                                                                                             | *models.PublishChildDeploymentRequestValidationUnion*                                                    | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |