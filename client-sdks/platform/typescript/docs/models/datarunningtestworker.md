# DataRunningTestWorker

## Example Usage

```typescript
import { DataRunningTestWorker } from "@alienplatform/platform-api/models";

let value: DataRunningTestWorker = {
  stackName: "<value>",
  type: "RunningTestWorker",
};
```

## Fields

| Field                          | Type                           | Required                       | Description                    |
| ------------------------------ | ------------------------------ | ------------------------------ | ------------------------------ |
| `stackName`                    | *string*                       | :heavy_check_mark:             | Name of the stack being tested |
| `type`                         | *"RunningTestWorker"*          | :heavy_check_mark:             | N/A                            |