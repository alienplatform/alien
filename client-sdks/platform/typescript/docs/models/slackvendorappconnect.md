# SlackVendorAppConnect

## Example Usage

```typescript
import { SlackVendorAppConnect } from "@alienplatform/platform-api/models";

let value: SlackVendorAppConnect = {
  botToken: "<value>",
  signingSecret: "<value>",
};
```

## Fields

| Field                                      | Type                                       | Required                                   | Description                                |
| ------------------------------------------ | ------------------------------------------ | ------------------------------------------ | ------------------------------------------ |
| `botToken`                                 | *string*                                   | :heavy_check_mark:                         | The app's Bot User OAuth Token (`xoxb-…`). |
| `signingSecret`                            | *string*                                   | :heavy_check_mark:                         | The app's Signing Secret.                  |