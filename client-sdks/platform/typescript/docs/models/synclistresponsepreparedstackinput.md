# SyncListResponsePreparedStackInput

Stack input definition serialized into a release stack.

## Example Usage

```typescript
import { SyncListResponsePreparedStackInput } from "@alienplatform/platform-api/models";

let value: SyncListResponsePreparedStackInput = {
  description: "or calculus vision via buttery times",
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
| `default`                                                                                                | *models.SyncListResponsePreparedStackDefaultUnion*                                                       | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `description`                                                                                            | *string*                                                                                                 | :heavy_check_mark:                                                                                       | Human-facing helper text.                                                                                |
| `env`                                                                                                    | [models.SyncListResponsePreparedStackEnv](../models/synclistresponsepreparedstackenv.md)[]               | :heavy_minus_sign:                                                                                       | Runtime env-var mappings for v1 input resolution.                                                        |
| `id`                                                                                                     | *string*                                                                                                 | :heavy_check_mark:                                                                                       | Stable input ID used by CLI/API calls.                                                                   |
| `kind`                                                                                                   | [models.SyncListResponsePreparedStackKind](../models/synclistresponsepreparedstackkind.md)               | :heavy_check_mark:                                                                                       | Primitive stack input kind.                                                                              |
| `label`                                                                                                  | *string*                                                                                                 | :heavy_check_mark:                                                                                       | Human-facing field label.                                                                                |
| `placeholder`                                                                                            | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | Example placeholder shown in UI.                                                                         |
| `platforms`                                                                                              | [models.SyncListResponsePreparedStackPlatform](../models/synclistresponsepreparedstackplatform.md)[]     | :heavy_minus_sign:                                                                                       | Platforms where this input applies.                                                                      |
| `providedBy`                                                                                             | [models.SyncListResponsePreparedStackProvidedBy](../models/synclistresponsepreparedstackprovidedby.md)[] | :heavy_check_mark:                                                                                       | Who can provide this value.                                                                              |
| `required`                                                                                               | *boolean*                                                                                                | :heavy_check_mark:                                                                                       | Whether a resolved value is required before deployment can proceed.                                      |
| `validation`                                                                                             | *models.SyncListResponsePreparedStackValidationUnion*                                                    | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
