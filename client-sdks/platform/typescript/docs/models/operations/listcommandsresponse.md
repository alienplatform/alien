# ListCommandsResponse

Paginated response

## Example Usage

```typescript
import { ListCommandsResponse } from "@aliendotdev/platform-api/models/operations";

let value: ListCommandsResponse = {
  items: [],
  nextCursor: "<value>",
};
```

## Fields

| Field                                                                       | Type                                                                        | Required                                                                    | Description                                                                 |
| --------------------------------------------------------------------------- | --------------------------------------------------------------------------- | --------------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| `items`                                                                     | [models.CommandListItemResponse](../../models/commandlistitemresponse.md)[] | :heavy_check_mark:                                                          | Items in this page                                                          |
| `nextCursor`                                                                | *string*                                                                    | :heavy_check_mark:                                                          | Cursor for the next page, null if last page                                 |