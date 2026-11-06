# CreateProjectFromTemplateDefaultManagers

Project default private managers for new push deployments.

## Example Usage

```typescript
import { CreateProjectFromTemplateDefaultManagers } from "@alienplatform/platform-api/models/operations";

let value: CreateProjectFromTemplateDefaultManagers = {
  aws: "mgr_enxscjrqiiu2lrc672hwwuc5tv5y",
  gcp: "mgr_enxscjrqiiu2lrc672hwwuc5tv5y",
  azure: "mgr_enxscjrqiiu2lrc672hwwuc5tv5y",
  kubernetes: "mgr_enxscjrqiiu2lrc672hwwuc5tv5y",
  machines: "mgr_enxscjrqiiu2lrc672hwwuc5tv5y",
  local: "mgr_enxscjrqiiu2lrc672hwwuc5tv5y",
};
```

## Fields

| Field                                             | Type                                              | Required                                          | Description                                       | Example                                           |
| ------------------------------------------------- | ------------------------------------------------- | ------------------------------------------------- | ------------------------------------------------- | ------------------------------------------------- |
| `aws`                                             | *string*                                          | :heavy_minus_sign:                                | Unique identifier for a default private instance. | mgr_enxscjrqiiu2lrc672hwwuc5tv5y                  |
| `gcp`                                             | *string*                                          | :heavy_minus_sign:                                | Unique identifier for a default private instance. | mgr_enxscjrqiiu2lrc672hwwuc5tv5y                  |
| `azure`                                           | *string*                                          | :heavy_minus_sign:                                | Unique identifier for a default private instance. | mgr_enxscjrqiiu2lrc672hwwuc5tv5y                  |
| `kubernetes`                                      | *string*                                          | :heavy_minus_sign:                                | Unique identifier for a default private instance. | mgr_enxscjrqiiu2lrc672hwwuc5tv5y                  |
| `machines`                                        | *string*                                          | :heavy_minus_sign:                                | Unique identifier for a default private instance. | mgr_enxscjrqiiu2lrc672hwwuc5tv5y                  |
| `local`                                           | *string*                                          | :heavy_minus_sign:                                | Unique identifier for a default private instance. | mgr_enxscjrqiiu2lrc672hwwuc5tv5y                  |