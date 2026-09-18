# ListPackagesRequest

## Example Usage

```typescript
import { ListPackagesRequest } from "@alienplatform/platform-api/models/operations";

let value: ListPackagesRequest = {};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `project`                                                                      | *string*                                                                       | :heavy_minus_sign:                                                             | Filter by project ID or name.                                                  |
| `type`                                                                         | [operations.ListPackagesType](../../models/operations/listpackagestype.md)     | :heavy_minus_sign:                                                             | Filter by package type                                                         |
| `status`                                                                       | [operations.ListPackagesStatus](../../models/operations/listpackagesstatus.md) | :heavy_minus_sign:                                                             | Filter by package status                                                       |
| `search`                                                                       | *string*                                                                       | :heavy_minus_sign:                                                             | Search packages by type or version                                             |
| `limit`                                                                        | *number*                                                                       | :heavy_minus_sign:                                                             | Maximum number of items to return per page                                     |
| `cursor`                                                                       | *string*                                                                       | :heavy_minus_sign:                                                             | Cursor for pagination - omit for first page                                    |