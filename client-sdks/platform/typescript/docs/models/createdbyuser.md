# CreatedByUser

User information associated with the API key

## Example Usage

```typescript
import { CreatedByUser } from "@alienplatform/platform-api/models";

let value: CreatedByUser = {
  id: "<id>",
  name: "<value>",
  email: "Carrie_Schneider@yahoo.com",
  image: "https://picsum.photos/seed/mKHgc/1448/97",
};
```

## Fields

| Field                                 | Type                                  | Required                              | Description                           |
| ------------------------------------- | ------------------------------------- | ------------------------------------- | ------------------------------------- |
| `id`                                  | *string*                              | :heavy_check_mark:                    | N/A                                   |
| `name`                                | *string*                              | :heavy_check_mark:                    | Display name, when the person set one |
| `email`                               | *string*                              | :heavy_check_mark:                    | N/A                                   |
| `image`                               | *string*                              | :heavy_check_mark:                    | N/A                                   |