# IssuedSetupCredential

A key that updates one installation for a few minutes, and its deadline.
Not `Debug`: it holds a key.

## Example Usage

```typescript
import { IssuedSetupCredential } from "@alienplatform/manager-api/models";

let value: IssuedSetupCredential = {
  expiresAt: new Date("2024-09-04T16:52:20.165Z"),
  token: "<value>",
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `expiresAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `token`                                                                                       | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
