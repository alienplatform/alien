# CommandBootstrapResponse

## Example Usage

```typescript
import { CommandBootstrapResponse } from "@alienplatform/platform-api/models";

let value: CommandBootstrapResponse = {
  managerUrl: "https://harmful-kick.biz/",
  token: "<value>",
  expiresAt: new Date("2025-03-18T21:23:24.172Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `managerUrl`                                                                                  | *string*                                                                                      | :heavy_check_mark:                                                                            | Current customer-facing manager URL                                                           |
| `token`                                                                                       | *string*                                                                                      | :heavy_check_mark:                                                                            | Short-lived command capability token                                                          |
| `expiresAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | Capability expiry in RFC 3339 format                                                          |
| `target`                                                                                      | [models.CommandReceiverBootstrapTarget](../models/commandreceiverbootstraptarget.md)          | :heavy_minus_sign:                                                                            | Resolved target identity; present only for receiver bootstrap                                 |