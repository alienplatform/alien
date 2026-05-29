# GetResourceDeploymentDetailEvent

## Example Usage

```typescript
import { GetResourceDeploymentDetailEvent } from "@alienplatform/platform-api/models/operations";

let value: GetResourceDeploymentDetailEvent = {
  eventIndex: 936170,
  observedAt: new Date("2026-04-28T12:54:28.842Z"),
  severity: "<value>",
  kind: "<value>",
  message: "<value>",
  source: null,
  platformStale: true,
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `eventIndex`                                                                                  | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `observedAt`                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `severity`                                                                                    | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `kind`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `message`                                                                                     | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `source`                                                                                      | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `platformStale`                                                                               | *boolean*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `provider`                                                                                    | *any*                                                                                         | :heavy_minus_sign:                                                                            | N/A                                                                                           |