# PendingChangesResponse

## Example Usage

```typescript
import { PendingChangesResponse } from "@alienplatform/manager-api/models";

let value: PendingChangesResponse = {
  items: [
    {
      basedOnVersion: 147256,
      body: {
        "key": "<value>",
        "key1": "<value>",
      },
      createdAt: "1716824244575",
      id: "<id>",
      projectId: "<id>",
      status: "approved",
      summary: "<value>",
    },
  ],
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `items`                                                              | [models.PendingChangeResponse](../models/pendingchangeresponse.md)[] | :heavy_check_mark:                                                   | N/A                                                                  |
