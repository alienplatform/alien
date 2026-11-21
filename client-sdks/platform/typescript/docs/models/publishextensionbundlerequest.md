# PublishExtensionBundleRequest

## Example Usage

```typescript
import { PublishExtensionBundleRequest } from "@alienplatform/platform-api/models";

let value: PublishExtensionBundleRequest = {
  extension: "m1v",
  digest: "<value>",
  uploads: [
    {
      sha256: "<value>",
      uploadId: "47dcc9ce-89e8-4da1-9455-b0f100dd6b89",
    },
  ],
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `extension`                                                                                      | *string*                                                                                         | :heavy_check_mark:                                                                               | The extension's name, as its bundle's manifest declares it.                                      |
| `digest`                                                                                         | *string*                                                                                         | :heavy_check_mark:                                                                               | The core zip's digest, `sha256:<hex>`, as the release's stack pins it.                           |
| `uploads`                                                                                        | [models.PublishExtensionBundleRequestUpload](../models/publishextensionbundlerequestupload.md)[] | :heavy_check_mark:                                                                               | What step 1 asked to upload, by sha256 and upload id.                                            |