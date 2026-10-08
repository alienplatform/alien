# RemoteSandboxObservation

## Example Usage

```typescript
import { RemoteSandboxObservation } from "@alienplatform/platform-api/models";

let value: RemoteSandboxObservation = {
  provider: "<value>",
  platform: null,
  resourceId: "<id>",
  location: "<value>",
  account: "24746796",
  observedAt: new Date("2025-02-13T07:01:34.821Z"),
  stale: true,
  partial: false,
  health: "unhealthy",
  lifecycle: "<value>",
  message: "<value>",
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `provider`                                                                                    | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `platform`                                                                                    | [models.RemoteSandboxPlatform](../models/remotesandboxplatform.md)                            | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `resourceId`                                                                                  | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `location`                                                                                    | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `account`                                                                                     | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `observedAt`                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `stale`                                                                                       | *boolean*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `partial`                                                                                     | *boolean*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `health`                                                                                      | [models.RemoteSandboxHealth](../models/remotesandboxhealth.md)                                | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `lifecycle`                                                                                   | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `message`                                                                                     | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |