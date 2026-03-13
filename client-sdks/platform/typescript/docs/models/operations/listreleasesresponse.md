# ListReleasesResponse

Paginated response

## Example Usage

```typescript
import { ListReleasesResponse } from "@aliendotdev/platform-api/models/operations";

let value: ListReleasesResponse = {
  items: [],
  nextCursor: "<value>",
};
```

## Fields

| Field                                                                       | Type                                                                        | Required                                                                    | Description                                                                 |
| --------------------------------------------------------------------------- | --------------------------------------------------------------------------- | --------------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| `items`                                                                     | [models.ReleaseListItemResponse](../../models/releaselistitemresponse.md)[] | :heavy_check_mark:                                                          | Items in this page                                                          |
| `nextCursor`                                                                | *string*                                                                    | :heavy_check_mark:                                                          | Cursor for the next page, null if last page                                 |