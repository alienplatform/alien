# ReleaseListItemResponseCreatedBy

Platform user who created the release, included when ?include=createdBy is used

## Example Usage

```typescript
import { ReleaseListItemResponseCreatedBy } from "@alienplatform/platform-api/models";

let value: ReleaseListItemResponseCreatedBy = {
  id: "<id>",
  name: "<value>",
  email: "Timmothy4@yahoo.com",
  image: "https://picsum.photos/seed/TVwGWpOmO/2713/1619",
};
```

## Fields

| Field                   | Type                    | Required                | Description             |
| ----------------------- | ----------------------- | ----------------------- | ----------------------- |
| `id`                    | *string*                | :heavy_check_mark:      | User ID                 |
| `name`                  | *string*                | :heavy_check_mark:      | User's display name     |
| `email`                 | *string*                | :heavy_check_mark:      | User's email address    |
| `image`                 | *string*                | :heavy_check_mark:      | User's avatar image URL |
