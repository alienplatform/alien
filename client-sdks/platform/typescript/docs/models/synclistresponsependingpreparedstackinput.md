# SyncListResponsePendingPreparedStackInput

Stack input definition serialized into a release stack.

## Example Usage

```typescript
import { SyncListResponsePendingPreparedStackInput } from "@alienplatform/platform-api/models";

let value: SyncListResponsePendingPreparedStackInput = {
  description: "and ack twine absent plus",
  id: "<id>",
  kind: "enum",
  label: "<value>",
  providedBy: [],
  required: false,
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `default`                                                                                                              | *models.SyncListResponsePendingPreparedStackDefaultUnion*                                                              | :heavy_minus_sign:                                                                                                     | N/A                                                                                                                    |
| `description`                                                                                                          | *string*                                                                                                               | :heavy_check_mark:                                                                                                     | Human-facing helper text.                                                                                              |
| `env`                                                                                                                  | [models.SyncListResponsePendingPreparedStackEnv](../models/synclistresponsependingpreparedstackenv.md)[]               | :heavy_minus_sign:                                                                                                     | Runtime env-var mappings for v1 input resolution.                                                                      |
| `id`                                                                                                                   | *string*                                                                                                               | :heavy_check_mark:                                                                                                     | Stable input ID used by CLI/API calls.                                                                                 |
| `kind`                                                                                                                 | [models.SyncListResponsePendingPreparedStackKind](../models/synclistresponsependingpreparedstackkind.md)               | :heavy_check_mark:                                                                                                     | Primitive stack input kind.                                                                                            |
| `label`                                                                                                                | *string*                                                                                                               | :heavy_check_mark:                                                                                                     | Human-facing field label.                                                                                              |
| `placeholder`                                                                                                          | *string*                                                                                                               | :heavy_minus_sign:                                                                                                     | Example placeholder shown in UI.                                                                                       |
| `platforms`                                                                                                            | [models.SyncListResponsePendingPreparedStackPlatform](../models/synclistresponsependingpreparedstackplatform.md)[]     | :heavy_minus_sign:                                                                                                     | Platforms where this input applies.                                                                                    |
| `providedBy`                                                                                                           | [models.SyncListResponsePendingPreparedStackProvidedBy](../models/synclistresponsependingpreparedstackprovidedby.md)[] | :heavy_check_mark:                                                                                                     | Who can provide this value.                                                                                            |
| `required`                                                                                                             | *boolean*                                                                                                              | :heavy_check_mark:                                                                                                     | Whether a resolved value is required before deployment can proceed.                                                    |
| `validation`                                                                                                           | *models.SyncListResponsePendingPreparedStackValidationUnion*                                                           | :heavy_minus_sign:                                                                                                     | N/A                                                                                                                    |
