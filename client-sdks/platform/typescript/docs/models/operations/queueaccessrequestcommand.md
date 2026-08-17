# QueueAccessRequestCommand

## Example Usage

```typescript
import { QueueAccessRequestCommand } from "@alienplatform/platform-api/models/operations";

let value: QueueAccessRequestCommand = {
  command: "kubernetes/get-pods",
  summary: "List pods in the ingestion namespace",
};
```

## Fields

| Field                                | Type                                 | Required                             | Description                          | Example                              |
| ------------------------------------ | ------------------------------------ | ------------------------------------ | ------------------------------------ | ------------------------------------ |
| `command`                            | *string*                             | :heavy_check_mark:                   | N/A                                  | kubernetes/get-pods                  |
| `summary`                            | *string*                             | :heavy_check_mark:                   | N/A                                  | List pods in the ingestion namespace |