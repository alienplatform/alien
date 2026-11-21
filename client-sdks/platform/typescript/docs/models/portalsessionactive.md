# PortalSessionActive

## Example Usage

```typescript
import { PortalSessionActive } from "@alienplatform/platform-api/models";

let value: PortalSessionActive = {
  state: "active",
  token: "<value>",
  expiresAt: new Date("2025-09-29T02:31:38.189Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `state`                                                                                       | *"active"*                                                                                    | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `token`                                                                                       | *string*                                                                                      | :heavy_check_mark:                                                                            | Bearer token for the deployment portal's API calls                                            |
| `expiresAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |