# GetAccessRequestCoordinatesResponse

The approve command (or null) and current status.

## Example Usage

```typescript
import { GetAccessRequestCoordinatesResponse } from "@alienplatform/platform-api/models/operations";

let value: GetAccessRequestCoordinatesResponse = {
  status: "rejected",
  kubectlApprove: "<value>",
};
```

## Fields

| Field                                                             | Type                                                              | Required                                                          | Description                                                       |
| ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- |
| `status`                                                          | [models.AccessRequestStatus](../../models/accessrequeststatus.md) | :heavy_check_mark:                                                | N/A                                                               |
| `kubectlApprove`                                                  | *string*                                                          | :heavy_check_mark:                                                | N/A                                                               |