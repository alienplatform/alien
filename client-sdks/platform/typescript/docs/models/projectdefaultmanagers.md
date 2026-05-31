# ProjectDefaultManagers

Project default private managers for new push deployments.

## Example Usage

```typescript
import { ProjectDefaultManagers } from "@alienplatform/platform-api/models";

let value: ProjectDefaultManagers = {
  aws: "mgr_enxscjrqiiu2lrc672hwwuc5",
  gcp: "mgr_enxscjrqiiu2lrc672hwwuc5",
  azure: "mgr_enxscjrqiiu2lrc672hwwuc5",
  kubernetes: "mgr_enxscjrqiiu2lrc672hwwuc5",
  local: "mgr_enxscjrqiiu2lrc672hwwuc5",
};
```

## Fields

| Field                                            | Type                                             | Required                                         | Description                                      | Example                                          |
| ------------------------------------------------ | ------------------------------------------------ | ------------------------------------------------ | ------------------------------------------------ | ------------------------------------------------ |
| `aws`                                            | *string*                                         | :heavy_minus_sign:                               | Unique identifier for a default private manager. | mgr_enxscjrqiiu2lrc672hwwuc5                     |
| `gcp`                                            | *string*                                         | :heavy_minus_sign:                               | Unique identifier for a default private manager. | mgr_enxscjrqiiu2lrc672hwwuc5                     |
| `azure`                                          | *string*                                         | :heavy_minus_sign:                               | Unique identifier for a default private manager. | mgr_enxscjrqiiu2lrc672hwwuc5                     |
| `kubernetes`                                     | *string*                                         | :heavy_minus_sign:                               | Unique identifier for a default private manager. | mgr_enxscjrqiiu2lrc672hwwuc5                     |
| `local`                                          | *string*                                         | :heavy_minus_sign:                               | Unique identifier for a default private manager. | mgr_enxscjrqiiu2lrc672hwwuc5                     |