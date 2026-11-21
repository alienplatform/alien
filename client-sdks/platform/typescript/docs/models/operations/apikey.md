# ApiKey

## Example Usage

```typescript
import { ApiKey } from "@alienplatform/platform-api/models/operations";

let value: ApiKey = {
  id: "<id>",
  name: "<value>",
};
```

## Fields

| Field                                                 | Type                                                  | Required                                              | Description                                           |
| ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- |
| `id`                                                  | *string*                                              | :heavy_check_mark:                                    | N/A                                                   |
| `name`                                                | *string*                                              | :heavy_check_mark:                                    | The key's description, or its prefix when it has none |