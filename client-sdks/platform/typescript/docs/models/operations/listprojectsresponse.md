# ListProjectsResponse

Paginated response

## Example Usage

```typescript
import { ListProjectsResponse } from "@alienplatform/platform-api/models/operations";

let value: ListProjectsResponse = {
  items: [],
  nextCursor: null,
};
```

## Fields

| Field                                                                       | Type                                                                        | Required                                                                    | Description                                                                 |
| --------------------------------------------------------------------------- | --------------------------------------------------------------------------- | --------------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| `items`                                                                     | [models.ProjectListItemResponse](../../models/projectlistitemresponse.md)[] | :heavy_check_mark:                                                          | Items in this page                                                          |
| `nextCursor`                                                                | *string*                                                                    | :heavy_check_mark:                                                          | Cursor for the next page, null if last page                                 |