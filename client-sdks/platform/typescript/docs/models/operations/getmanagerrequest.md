# GetManagerRequest

## Example Usage

```typescript
import { GetManagerRequest } from "@alienplatform/platform-api/models/operations";

let value: GetManagerRequest = {
  id: "mgr_enxscjrqiiu2lrc672hwwuc5tv5y",
};
```

## Fields

| Field                                                                                                                                         | Type                                                                                                                                          | Required                                                                                                                                      | Description                                                                                                                                   | Example                                                                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- |
| `id`                                                                                                                                          | *string*                                                                                                                                      | :heavy_check_mark:                                                                                                                            | Unique identifier for a manager.                                                                                                              | mgr_enxscjrqiiu2lrc672hwwuc5tv5y                                                                                                              |
| `includeCounts`                                                                                                                               | [operations.IncludeCounts](../../models/operations/includecounts.md)                                                                          | :heavy_minus_sign:                                                                                                                            | Count the deployments and projects that use this manager. On by default; with "false" the lookup skips the counts and both are returned as 0. |                                                                                                                                               |