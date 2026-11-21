# GetExtensionBundleDownloadUrlRequest

## Example Usage

```typescript
import { GetExtensionBundleDownloadUrlRequest } from "@alienplatform/platform-api/models/operations";

let value: GetExtensionBundleDownloadUrlRequest = {
  digest: "<value>",
  project: "<value>",
};
```

## Fields

| Field                                | Type                                 | Required                             | Description                          |
| ------------------------------------ | ------------------------------------ | ------------------------------------ | ------------------------------------ |
| `digest`                             | *string*                             | :heavy_check_mark:                   | The object's digest, `sha256:<hex>`. |
| `project`                            | *string*                             | :heavy_check_mark:                   | Filter by project ID or name.        |