# ListWorkspacesResponse

Paginated response

## Example Usage

```typescript
import { ListWorkspacesResponse } from "@aliendotdev/platform-api/models/operations";

let value: ListWorkspacesResponse = {
  items: [],
  nextCursor: "<value>",
};
```

## Fields

| Field                                           | Type                                            | Required                                        | Description                                     |
| ----------------------------------------------- | ----------------------------------------------- | ----------------------------------------------- | ----------------------------------------------- |
| `items`                                         | [models.Workspace](../../models/workspace.md)[] | :heavy_check_mark:                              | Items in this page                              |
| `nextCursor`                                    | *string*                                        | :heavy_check_mark:                              | Cursor for the next page, null if last page     |