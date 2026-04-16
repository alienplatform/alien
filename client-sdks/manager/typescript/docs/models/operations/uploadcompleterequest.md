# UploadCompleteRequest

## Example Usage

```typescript
import { UploadCompleteRequest } from "@alienplatform/manager-api/models/operations";

let value: UploadCompleteRequest = {
  commandId: "<id>",
  uploadCompleteRequest: {
    size: 219776,
  },
};
```

## Fields

| Field                                                                 | Type                                                                  | Required                                                              | Description                                                           |
| --------------------------------------------------------------------- | --------------------------------------------------------------------- | --------------------------------------------------------------------- | --------------------------------------------------------------------- |
| `commandId`                                                           | *string*                                                              | :heavy_check_mark:                                                    | Command identifier                                                    |
| `uploadCompleteRequest`                                               | [models.UploadCompleteRequest](../../models/uploadcompleterequest.md) | :heavy_check_mark:                                                    | N/A                                                                   |