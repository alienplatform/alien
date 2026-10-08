# GetLogExportDeliveryConfigResponse

Private runtime configuration for the assigned manager.

## Example Usage

```typescript
import { GetLogExportDeliveryConfigResponse } from "@alienplatform/platform-api/models/operations";

let value: GetLogExportDeliveryConfigResponse = {
  endpoint: "<value>",
  enabled: false,
  headers: {
    "key": "<value>",
    "key1": "<value>",
  },
  revision: "<value>",
  updatedAt: new Date("2026-08-08T15:38:37.175Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `endpoint`                                                                                    | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `enabled`                                                                                     | *boolean*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `headers`                                                                                     | Record<string, *string*>                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `revision`                                                                                    | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `updatedAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
