# LocalRuntimeEventSnapshot

## Example Usage

```typescript
import { LocalRuntimeEventSnapshot } from "@alienplatform/manager-api/models";

let value: LocalRuntimeEventSnapshot = {
  kind: "<value>",
  message: "<value>",
  severity: "error",
  timestamp: new Date("2025-05-23T09:04:10.937Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `kind`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `message`                                                                                     | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `raw`                                                                                         | *any*                                                                                         | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `severity`                                                                                    | [models.HeartbeatIssueSeverity](../models/heartbeatissueseverity.md)                          | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `subject`                                                                                     | [models.LocalRuntimeEventSubject](../models/localruntimeeventsubject.md)                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `timestamp`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |