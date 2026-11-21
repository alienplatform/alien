# PendingChangesResponse

## Example Usage

```typescript
import { PendingChangesResponse } from "@alienplatform/platform-api/models";

let value: PendingChangesResponse = {
  items: [
    {
      id: "pch_gz7jg0o8yf5m0b0h8uanp2tajfon",
      projectId: "<id>",
      status: "pending",
      summary: "<value>",
      basedOnVersion: 640565,
      body: {
        "key": "<value>",
      },
      requestedBy: "<value>",
      createdAt: new Date("2025-12-10T13:13:38.460Z"),
      resolvedAt: new Date("2024-12-30T16:12:22.769Z"),
      signerListVersion: null,
    },
  ],
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `items`                                              | [models.PendingChange](../models/pendingchange.md)[] | :heavy_check_mark:                                   | Pending changes, newest first                        |