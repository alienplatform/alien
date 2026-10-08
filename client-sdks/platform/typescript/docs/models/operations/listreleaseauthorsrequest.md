# ListReleaseAuthorsRequest

## Example Usage

```typescript
import { ListReleaseAuthorsRequest } from "@alienplatform/platform-api/models/operations";

let value: ListReleaseAuthorsRequest = {};
```

## Fields

| Field                                                       | Type                                                        | Required                                                    | Description                                                 |
| ----------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------- |
| `project`                                                   | *string*                                                    | :heavy_minus_sign:                                          | Filter by project ID or name.                               |
| `search`                                                    | *string*                                                    | :heavy_minus_sign:                                          | Search authors by login or name (case-insensitive contains) |
| `limit`                                                     | *number*                                                    | :heavy_minus_sign:                                          | Maximum number of authors to return                         |