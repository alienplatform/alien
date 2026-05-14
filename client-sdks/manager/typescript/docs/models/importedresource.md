# ImportedResource

One resolved resource import payload.

## Example Usage

```typescript
import { ImportedResource } from "@alienplatform/manager-api/models";

let value: ImportedResource = {
  id: "<id>",
  importData: {},
  type: "function",
};
```

## Fields

| Field                                                                                                                                                      | Type                                                                                                                                                       | Required                                                                                                                                                   | Description                                                                                                                                                | Example                                                                                                                                                    |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `id`                                                                                                                                                       | *string*                                                                                                                                                   | :heavy_check_mark:                                                                                                                                         | Resource id from the active stack.                                                                                                                         |                                                                                                                                                            |
| `importData`                                                                                                                                               | [models.ImportData](../models/importdata.md)                                                                                                               | :heavy_check_mark:                                                                                                                                         | Resolved typed payload for this resource.                                                                                                                  |                                                                                                                                                            |
| `type`                                                                                                                                                     | *string*                                                                                                                                                   | :heavy_check_mark:                                                                                                                                         | Resource type identifier that determines the specific kind of resource. This field is used for polymorphic deserialization and resource-specific behavior. | function                                                                                                                                                   |