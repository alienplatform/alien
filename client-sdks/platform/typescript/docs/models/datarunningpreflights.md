# DataRunningPreflights

## Example Usage

```typescript
import { DataRunningPreflights } from "@aliendotdev/platform-api/models";

let value: DataRunningPreflights = {
  platform: "<value>",
  stack: "<value>",
  type: "RunningPreflights",
};
```

## Fields

| Field                           | Type                            | Required                        | Description                     |
| ------------------------------- | ------------------------------- | ------------------------------- | ------------------------------- |
| `platform`                      | *string*                        | :heavy_check_mark:              | Platform being targeted         |
| `stack`                         | *string*                        | :heavy_check_mark:              | Name of the stack being checked |
| `type`                          | *"RunningPreflights"*           | :heavy_check_mark:              | N/A                             |