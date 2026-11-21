# CreatePortalSessionRequest

## Example Usage

```typescript
import { CreatePortalSessionRequest } from "@alienplatform/platform-api/models";

let value: CreatePortalSessionRequest = {
  link: "<value>",
};
```

## Fields

| Field                                      | Type                                       | Required                                   | Description                                |
| ------------------------------------------ | ------------------------------------------ | ------------------------------------------ | ------------------------------------------ |
| `link`                                     | *string*                                   | :heavy_check_mark:                         | The code after # in the setup link URL     |
| `email`                                    | *string*                                   | :heavy_minus_sign:                         | The person's email, for links that ask one |
| `code`                                     | *string*                                   | :heavy_minus_sign:                         | The code emailed to that address           |