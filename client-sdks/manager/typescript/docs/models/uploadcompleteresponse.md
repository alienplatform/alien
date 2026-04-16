# UploadCompleteResponse

Response to upload completion

## Example Usage

```typescript
import { UploadCompleteResponse } from "@alienplatform/manager-api/models";

let value: UploadCompleteResponse = {
  commandId: "<id>",
  state: "EXPIRED",
};
```

## Fields

| Field                                             | Type                                              | Required                                          | Description                                       |
| ------------------------------------------------- | ------------------------------------------------- | ------------------------------------------------- | ------------------------------------------------- |
| `commandId`                                       | *string*                                          | :heavy_check_mark:                                | Command identifier                                |
| `state`                                           | [models.CommandState](../models/commandstate.md)  | :heavy_check_mark:                                | Command states in the Commands protocol lifecycle |