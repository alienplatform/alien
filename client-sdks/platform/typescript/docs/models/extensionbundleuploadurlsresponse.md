# ExtensionBundleUploadUrlsResponse

## Example Usage

```typescript
import { ExtensionBundleUploadUrlsResponse } from "@alienplatform/platform-api/models";

let value: ExtensionBundleUploadUrlsResponse = {
  uploads: [],
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `uploads`                                                                                                | [models.ExtensionBundleUploadUrlsResponseUpload](../models/extensionbundleuploadurlsresponseupload.md)[] | :heavy_check_mark:                                                                                       | One upload per object the project does not have yet. Empty when it has all.                              |