# ListAPIKeysRequest

## Example Usage

```typescript
import { ListAPIKeysRequest } from "@alienplatform/platform-api/models/operations";

let value: ListAPIKeysRequest = {};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `project`                                                                | *string*                                                                 | :heavy_minus_sign:                                                       | Filter by project ID or name.                                            |
| `setupLinksForGroup`                                                     | *string*                                                                 | :heavy_minus_sign:                                                       | List setup links for this deployment group, excluding dashboard sessions |
| `limit`                                                                  | *number*                                                                 | :heavy_minus_sign:                                                       | Maximum number of items to return per page                               |
| `cursor`                                                                 | *string*                                                                 | :heavy_minus_sign:                                                       | Cursor for pagination - omit for first page                              |