# RecentLogEntry

## Example Usage

```typescript
import { RecentLogEntry } from "@alienplatform/manager-api/models";

let value: RecentLogEntry = {
  attributes: {
    "key": "<value>",
    "key1": "<value>",
  },
  message: "<value>",
  severity: "<value>",
  timestamp: new Date("2024-03-08T08:40:29.021Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `attributes`                                                                                  | Record<string, *string*>                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `message`                                                                                     | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `resource`                                                                                    | *string*                                                                                      | :heavy_minus_sign:                                                                            | Workload that produced the entry (OTLP `service.name`).                                       |
| `severity`                                                                                    | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `timestamp`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |