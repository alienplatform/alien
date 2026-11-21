# ExtensionBundleUploadUrlsRequest

## Example Usage

```typescript
import { ExtensionBundleUploadUrlsRequest } from "@alienplatform/platform-api/models";

let value: ExtensionBundleUploadUrlsRequest = {
  extension: "png",
  objects: [
    {
      object: "<value>",
      sha256: "<value>",
      size: 207348,
    },
  ],
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `extension`                                                          | *string*                                                             | :heavy_check_mark:                                                   | The extension's name, as its bundle's manifest declares it.          |
| `objects`                                                            | [models.ExtensionBundleObject](../models/extensionbundleobject.md)[] | :heavy_check_mark:                                                   | Every object of the bundle.                                          |