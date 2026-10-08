# AcceptRemoteOperatorImageResponse

The observed immutable image is now the accepted desired image.

## Example Usage

```typescript
import { AcceptRemoteOperatorImageResponse } from "@alienplatform/platform-api/models/operations";

let value: AcceptRemoteOperatorImageResponse = {
  operatorImage: {
    source: "package",
    packageId: "<id>",
    packageVersion: "<value>",
    image: "https://picsum.photos/seed/msGGlsLY/2206/1710",
    digest: "<value>",
    renderedAt: new Date("2025-12-05T21:59:46.408Z"),
  },
};
```

## Fields

| Field                                                                               | Type                                                                                | Required                                                                            | Description                                                                         |
| ----------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------- |
| `operatorImage`                                                                     | [models.RemoteOperatorInstallReceipt](../../models/remoteoperatorinstallreceipt.md) | :heavy_check_mark:                                                                  | N/A                                                                                 |