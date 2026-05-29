# GetResourceDeploymentDetailEvent

## Example Usage

```typescript
import { GetResourceDeploymentDetailEvent } from "@alienplatform/platform-api/models/operations";

let value: GetResourceDeploymentDetailEvent = {
  eventId: "<id>",
  eventIndex: 774214,
  observedAt: new Date("2024-04-08T01:22:09.580Z"),
  severity: "<value>",
  kind: "<value>",
  message: "<value>",
  source: "<value>",
  subjectKind: "machine",
  subjectId: "<id>",
  subjectName: "<value>",
  platformStale: true,
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `eventId`                                                                                     | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `eventIndex`                                                                                  | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `observedAt`                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `severity`                                                                                    | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `kind`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `message`                                                                                     | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `source`                                                                                      | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `subjectKind`                                                                                 | [operations.SubjectKind](../../models/operations/subjectkind.md)                              | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `subjectId`                                                                                   | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `subjectName`                                                                                 | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `platformStale`                                                                               | *boolean*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `providerEvent`                                                                               | *any*                                                                                         | :heavy_minus_sign:                                                                            | N/A                                                                                           |