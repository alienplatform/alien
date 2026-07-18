# ResolvedCommandTargetTarget

Identifies the specific resource a command is addressed to.

## Example Usage

```typescript
import { ResolvedCommandTargetTarget } from "@alienplatform/platform-api/models";

let value: ResolvedCommandTargetTarget = {
  resourceId: "<id>",
  resourceType: "daemon",
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `resourceId`                                                                               | *string*                                                                                   | :heavy_check_mark:                                                                         | The resource ID within the deployment's stack (e.g. a Worker/Container/Daemon id).         |
| `resourceType`                                                                             | [models.ResolvedCommandTargetResourceType](../models/resolvedcommandtargetresourcetype.md) | :heavy_check_mark:                                                                         | The kind of command-capable resource a command targets.                                    |