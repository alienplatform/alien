# PromoteReleaseRequest

## Example Usage

```typescript
import { PromoteReleaseRequest } from "@alienplatform/platform-api/models/operations";

let value: PromoteReleaseRequest = {
  name: "<value>",
  project: "<value>",
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `name`                                                                                       | *string*                                                                                     | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `project`                                                                                    | *string*                                                                                     | :heavy_check_mark:                                                                           | Filter by project ID or name.                                                                |
| `requestBody`                                                                                | [operations.PromoteReleaseRequestBody](../../models/operations/promotereleaserequestbody.md) | :heavy_minus_sign:                                                                           | N/A                                                                                          |