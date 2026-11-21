# PortalInstallerKey

## Example Usage

```typescript
import { PortalInstallerKey } from "@alienplatform/platform-api/models";

let value: PortalInstallerKey = {
  token: "<value>",
  expiresAt: new Date("2024-08-07T16:02:24.182Z"),
};
```

## Fields

| Field                                                                                               | Type                                                                                                | Required                                                                                            | Description                                                                                         |
| --------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| `token`                                                                                             | *string*                                                                                            | :heavy_check_mark:                                                                                  | A deployer API key that acts for the setup link                                                     |
| `expiresAt`                                                                                         | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)       | :heavy_check_mark:                                                                                  | When the key expires, 24 hours after it was minted. It also stops working when the setup link does. |