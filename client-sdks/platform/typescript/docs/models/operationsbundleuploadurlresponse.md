# OperationsBundleUploadUrlResponse

## Example Usage

```typescript
import { OperationsBundleUploadUrlResponse } from "@alienplatform/platform-api/models";

let value: OperationsBundleUploadUrlResponse = {
  uploadUrl: "https://legal-meadow.com/",
  uploadId: "<id>",
  contentType: "<value>",
};
```

## Fields

| Field                                                                                                                                          | Type                                                                                                                                           | Required                                                                                                                                       | Description                                                                                                                                    |
| ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| `uploadUrl`                                                                                                                                    | *string*                                                                                                                                       | :heavy_check_mark:                                                                                                                             | Presigned S3 PUT URL to upload the bundle ZIP to.                                                                                              |
| `uploadId`                                                                                                                                     | *string*                                                                                                                                       | :heavy_check_mark:                                                                                                                             | One-time id for this upload. Pass it back verbatim to POST /plugins so the API can validate and snapshot exactly the bytes this call uploaded. |
| `contentType`                                                                                                                                  | *string*                                                                                                                                       | :heavy_check_mark:                                                                                                                             | Content-Type header the PUT must send (must match the signature).                                                                              |