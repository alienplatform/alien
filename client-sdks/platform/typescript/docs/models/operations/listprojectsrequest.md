# ListProjectsRequest

## Example Usage

```typescript
import { ListProjectsRequest } from "@alienplatform/platform-api/models/operations";

let value: ListProjectsRequest = {};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `search`                                                                           | *string*                                                                           | :heavy_minus_sign:                                                                 | Search projects by name                                                            |
| `include`                                                                          | [operations.ListProjectsInclude](../../models/operations/listprojectsinclude.md)[] | :heavy_minus_sign:                                                                 | Optional fields to include: deploymentCount, latestRelease                         |
| `limit`                                                                            | *number*                                                                           | :heavy_minus_sign:                                                                 | Maximum number of items to return per page                                         |
| `cursor`                                                                           | *string*                                                                           | :heavy_minus_sign:                                                                 | Cursor for pagination - omit for first page                                        |