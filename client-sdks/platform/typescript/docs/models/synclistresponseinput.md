# SyncListResponseInput

Stack input definition serialized into a release stack.

## Example Usage

```typescript
import { SyncListResponseInput } from "@alienplatform/platform-api/models";

let value: SyncListResponseInput = {
  description: "draw neatly round modulo sensitize",
  id: "<id>",
  kind: "integer",
  label: "<value>",
  providedBy: [
    "deployer",
  ],
  required: false,
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `default`                                                                                                  | *models.SyncListResponseDefaultUnion*                                                                      | :heavy_minus_sign:                                                                                         | N/A                                                                                                        |
| `description`                                                                                              | *string*                                                                                                   | :heavy_check_mark:                                                                                         | Human-facing helper text.                                                                                  |
| `env`                                                                                                      | [models.SyncListResponseEnv](../models/synclistresponseenv.md)[]                                           | :heavy_minus_sign:                                                                                         | Runtime env-var mappings for v1 input resolution.                                                          |
| `id`                                                                                                       | *string*                                                                                                   | :heavy_check_mark:                                                                                         | Stable input ID used by CLI/API calls.                                                                     |
| `kind`                                                                                                     | [models.SyncListResponseKind](../models/synclistresponsekind.md)                                           | :heavy_check_mark:                                                                                         | Primitive stack input kind.                                                                                |
| `label`                                                                                                    | *string*                                                                                                   | :heavy_check_mark:                                                                                         | Human-facing field label.                                                                                  |
| `placeholder`                                                                                              | *string*                                                                                                   | :heavy_minus_sign:                                                                                         | Example placeholder shown in UI.                                                                           |
| `platforms`                                                                                                | [models.SyncListResponsePreparedStackPlatform](../models/synclistresponsepreparedstackplatform.md)[]       | :heavy_minus_sign:                                                                                         | Platforms where this input applies.                                                                        |
| `providedBy`                                                                                               | [models.SyncListResponseProvidedBy](../models/synclistresponseprovidedby.md)[]                             | :heavy_check_mark:                                                                                         | Who can provide this value.                                                                                |
| `required`                                                                                                 | *boolean*                                                                                                  | :heavy_check_mark:                                                                                         | Whether a resolved value is required before deployment can proceed.                                        |
| `setupMethods`                                                                                             | [models.SyncListResponsePreparedStackSetupMethod](../models/synclistresponsepreparedstacksetupmethod.md)[] | :heavy_minus_sign:                                                                                         | Setup methods where this input applies.                                                                    |
| `validation`                                                                                               | *models.SyncListResponseValidationUnion*                                                                   | :heavy_minus_sign:                                                                                         | N/A                                                                                                        |