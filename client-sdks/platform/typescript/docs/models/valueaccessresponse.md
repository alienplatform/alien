# ValueAccessResponse

## Example Usage

```typescript
import { ValueAccessResponse } from "@alienplatform/platform-api/models";

let value: ValueAccessResponse = {
  managerUrl: "https://trusty-punctuation.net/",
  token: "<value>",
  scope: "<value>",
  expiresAt: new Date("2026-10-13T11:38:32.448Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `managerUrl`                                                                                  | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `token`                                                                                       | *string*                                                                                      | :heavy_check_mark:                                                                            | Platform JWT for the manager's values API. Valid for 5 minutes.                               |
| `scope`                                                                                       | *string*                                                                                      | :heavy_check_mark:                                                                            | Value scope on the manager, such as dep:dep_123.                                              |
| `expiresAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |