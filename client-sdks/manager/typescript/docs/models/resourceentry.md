# ResourceEntry

## Example Usage

```typescript
import { ResourceEntry } from "@alienplatform/manager-api/models";

let value: ResourceEntry = {
  resourceType: "<value>",
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `publicEndpoints`                                                                | Record<string, [models.PublicEndpointOutput](../models/publicendpointoutput.md)> | :heavy_minus_sign:                                                               | N/A                                                                              |
| `publicUrl`                                                                      | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `resourceType`                                                                   | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |