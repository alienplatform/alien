# RemoteOperatorInstallReceipt

## Example Usage

```typescript
import { RemoteOperatorInstallReceipt } from "@alienplatform/platform-api/models";

let value: RemoteOperatorInstallReceipt = {
  source: "configured",
  packageId: "<id>",
  packageVersion: "<value>",
  image: "https://picsum.photos/seed/M53pwo/115/1125",
  digest: "<value>",
  renderedAt: new Date("2024-03-06T11:30:35.781Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `source`                                                                                      | [models.RemoteOperatorInstallReceiptSource](../models/remoteoperatorinstallreceiptsource.md)  | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `packageId`                                                                                   | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `packageVersion`                                                                              | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `image`                                                                                       | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `digest`                                                                                      | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `renderedAt`                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |