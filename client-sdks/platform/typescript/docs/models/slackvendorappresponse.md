# SlackVendorAppResponse

## Example Usage

```typescript
import { SlackVendorAppResponse } from "@alienplatform/platform-api/models";

let value: SlackVendorAppResponse = {
  app: {
    id: "<id>",
    status: "pending",
    name: "<value>",
    description: "flint closed impact whispered petticoat wretched edge",
    color: "silver",
    slackAppId: null,
    botUserId: "<id>",
    connectedByUserId: "<id>",
    connectedAt: "<value>",
    manifestUrl: "https://tempting-director.com",
    appSettingsUrl: "https://impractical-basket.net/",
    slackTeamName: "<value>",
  },
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `app`                                                | [models.SlackVendorApp](../models/slackvendorapp.md) | :heavy_check_mark:                                   | N/A                                                  |