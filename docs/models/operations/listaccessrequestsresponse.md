# ListAccessRequestsResponse

Paginated response

## Example Usage

```typescript
import { ListAccessRequestsResponse } from "@alienplatform/platform-api/models/operations";

let value: ListAccessRequestsResponse = {
  items: [],
  nextCursor: "<value>",
};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `items`                                                                                  | [operations.ListAccessRequestsItem](../../models/operations/listaccessrequestsitem.md)[] | :heavy_check_mark:                                                                       | Items in this page                                                                       |
| `nextCursor`                                                                             | *string*                                                                                 | :heavy_check_mark:                                                                       | Cursor for the next page, null if last page                                              |