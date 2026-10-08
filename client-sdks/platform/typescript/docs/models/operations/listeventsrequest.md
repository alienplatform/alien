# ListEventsRequest

## Example Usage

```typescript
import { ListEventsRequest } from "@alienplatform/platform-api/models/operations";

let value: ListEventsRequest = {};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `project`                                                                      | *string*                                                                       | :heavy_minus_sign:                                                             | Filter by project ID or name.                                                  |
| `deploymentId`                                                                 | *string*                                                                       | :heavy_minus_sign:                                                             | Filter events to a single deployment.                                          |
| `releaseId`                                                                    | *string*                                                                       | :heavy_minus_sign:                                                             | Filter events to a single release.                                             |
| `include`                                                                      | [operations.ListEventsInclude](../../models/operations/listeventsinclude.md)[] | :heavy_minus_sign:                                                             | Optional fields to include: releaseCreatedAt                                   |
| `limit`                                                                        | *number*                                                                       | :heavy_minus_sign:                                                             | Maximum number of items to return per page                                     |
| `cursor`                                                                       | *string*                                                                       | :heavy_minus_sign:                                                             | Cursor for pagination - omit for first page                                    |