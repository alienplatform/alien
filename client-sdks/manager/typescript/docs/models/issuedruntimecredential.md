# IssuedRuntimeCredential

Response credential and its hard deadline. Never log or persist a debug dump.

## Example Usage

```typescript
import { IssuedRuntimeCredential } from "@alienplatform/manager-api/models";

let value: IssuedRuntimeCredential = {
  expiresAt: new Date("2024-08-22T19:06:37.304Z"),
  token: "<value>",
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `expiresAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `token`                                                                                       | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
