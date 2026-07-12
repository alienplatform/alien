# Check

## Example Usage

```typescript
import { Check } from "@alienplatform/platform-api/models";

let value: Check = {
  code: "<value>",
  status: "unknown",
  message: "<value>",
  checkedAt: "<value>",
};
```

## Fields

| Field                                          | Type                                           | Required                                       | Description                                    |
| ---------------------------------------------- | ---------------------------------------------- | ---------------------------------------------- | ---------------------------------------------- |
| `code`                                         | *string*                                       | :heavy_check_mark:                             | N/A                                            |
| `status`                                       | [models.CheckStatus](../models/checkstatus.md) | :heavy_check_mark:                             | N/A                                            |
| `message`                                      | *string*                                       | :heavy_check_mark:                             | N/A                                            |
| `checkedAt`                                    | *string*                                       | :heavy_check_mark:                             | N/A                                            |