# AcceptRemoteOperatorImageRequestBody

## Example Usage

```typescript
import { AcceptRemoteOperatorImageRequestBody } from "@alienplatform/platform-api/models/operations";

let value: AcceptRemoteOperatorImageRequestBody = {
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