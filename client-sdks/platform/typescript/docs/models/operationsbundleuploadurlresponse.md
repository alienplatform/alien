# OperationsBundleUploadUrlResponse

## Example Usage

```typescript
import { OperationsBundleUploadUrlResponse } from "@alienplatform/platform-api/models";

let value: OperationsBundleUploadUrlResponse = {
  uploadUrl: "https://legal-meadow.com/",
  contentType: "<value>",
};
```

## Fields

| Field                                                             | Type                                                              | Required                                                          | Description                                                       |
| ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- |
| `uploadUrl`                                                       | *string*                                                          | :heavy_check_mark:                                                | Presigned S3 PUT URL to upload the bundle ZIP to.                 |
| `contentType`                                                     | *string*                                                          | :heavy_check_mark:                                                | Content-Type header the PUT must send (must match the signature). |