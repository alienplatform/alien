# DataGcpAgentPlatform

GCP: the Agent Platform template sessions are cut from, and the engine it hangs under.

No session count: that needs `aiplatform.sandboxEnvironments.list`, which only the management
permission set holds. No template state either — emission is gated on reading it `ACTIVE`,
which is what `status` already says.

## Example Usage

```typescript
import { DataGcpAgentPlatform } from "@alienplatform/platform-api/models/operations";

let value: DataGcpAgentPlatform = {
  engine: "<value>",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "stopping",
    partial: false,
    stale: false,
  },
  templateId: "<id>",
  backend: "gcpAgentPlatform",
};
```

## Fields

| Field                                                                                   | Type                                                                                    | Required                                                                                | Description                                                                             |
| --------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------- |
| `engine`                                                                                | *string*                                                                                | :heavy_check_mark:                                                                      | Reasoning engine the template hangs under, without which the template id names nothing. |
| `status`                                                                                | [operations.DataStatus75](../../models/operations/datastatus75.md)                      | :heavy_check_mark:                                                                      | N/A                                                                                     |
| `templateId`                                                                            | *string*                                                                                | :heavy_check_mark:                                                                      | The template sessions are currently cut from.                                           |
| `backend`                                                                               | *"gcpAgentPlatform"*                                                                    | :heavy_check_mark:                                                                      | N/A                                                                                     |