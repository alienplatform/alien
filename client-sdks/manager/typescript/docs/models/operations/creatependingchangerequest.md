# CreatePendingChangeRequest

## Example Usage

```typescript
import { CreatePendingChangeRequest } from "@alienplatform/manager-api/models/operations";

let value: CreatePendingChangeRequest = {
  project: "<value>",
  createPendingChangeRequest: {
    basedOnVersion: 217411,
    body: {
      "key": "<value>",
    },
    summary: "<value>",
  },
};
```

## Fields

| Field                                                                           | Type                                                                            | Required                                                                        | Description                                                                     |
| ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| `project`                                                                       | *string*                                                                        | :heavy_check_mark:                                                              | Project id                                                                      |
| `createPendingChangeRequest`                                                    | [models.CreatePendingChangeRequest](../../models/creatependingchangerequest.md) | :heavy_check_mark:                                                              | N/A                                                                             |
