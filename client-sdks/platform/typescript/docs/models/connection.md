# Connection

## Example Usage

```typescript
import { Connection } from "@alienplatform/platform-api/models";

let value: Connection = {
  managerUrl: "https://grizzled-ecliptic.net/",
  token: "<value>",
  command: "<value>",
  targetResourceId: "<id>",
  idempotencyKey: "<value>",
  deadline: new Date("2025-09-29T20:35:55.277Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `managerUrl`                                                                                  | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `token`                                                                                       | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `command`                                                                                     | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `targetResourceId`                                                                            | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `idempotencyKey`                                                                              | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `deadline`                                                                                    | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `operationResultContract`                                                                     | *any*                                                                                         | :heavy_minus_sign:                                                                            | N/A                                                                                           |