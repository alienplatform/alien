# ExtensionBundleUploadUrlsResponseUpload

## Example Usage

```typescript
import { ExtensionBundleUploadUrlsResponseUpload } from "@alienplatform/platform-api/models";

let value: ExtensionBundleUploadUrlsResponseUpload = {
  object: "<value>",
  sha256: "<value>",
  uploadId: "<id>",
  uploadUrl: "https://portly-overcoat.com",
  contentType: "<value>",
};
```

## Fields

| Field                                                 | Type                                                  | Required                                              | Description                                           |
| ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- |
| `object`                                              | *string*                                              | :heavy_check_mark:                                    | N/A                                                   |
| `sha256`                                              | *string*                                              | :heavy_check_mark:                                    | The object's sha256, as 64 lowercase hex characters.  |
| `uploadId`                                            | *string*                                              | :heavy_check_mark:                                    | Pass back to POST /bundles with this object's sha256. |
| `uploadUrl`                                           | *string*                                              | :heavy_check_mark:                                    | Presigned S3 PUT URL for the object's bytes.          |
| `contentType`                                         | *string*                                              | :heavy_check_mark:                                    | Content-Type the PUT must send.                       |