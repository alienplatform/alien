# CreatedBy

Platform user who created the release, included when ?include=createdBy is used

## Example Usage

```typescript
import { CreatedBy } from "@alienplatform/platform-api/models";

let value: CreatedBy = {
  id: "<id>",
  name: "<value>",
  email: "Jacky.Marvin@yahoo.com",
  image: "https://loremflickr.com/3225/676?lock=2601971394296232",
};
```

## Fields

| Field                   | Type                    | Required                | Description             |
| ----------------------- | ----------------------- | ----------------------- | ----------------------- |
| `id`                    | *string*                | :heavy_check_mark:      | User ID                 |
| `name`                  | *string*                | :heavy_check_mark:      | User's display name     |
| `email`                 | *string*                | :heavy_check_mark:      | User's email address    |
| `image`                 | *string*                | :heavy_check_mark:      | User's avatar image URL |
