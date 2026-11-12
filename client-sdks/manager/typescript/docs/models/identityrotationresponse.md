# IdentityRotationResponse

No successor credential or hash is disclosed to a retired key holder.

## Example Usage

```typescript
import { IdentityRotationResponse } from "@alienplatform/manager-api/models";

let value: IdentityRotationResponse = {
  expiresAt: new Date("2026-04-27T19:59:03.968Z"),
  revision: 169623,
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `expiresAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `revision`                                                                                    | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
