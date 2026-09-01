# CreateAccessRequest

## Example Usage

```typescript
import { CreateAccessRequest } from "@alienplatform/platform-api/models";

let value: CreateAccessRequest = {
  deploymentId: "<id>",
  commands: [
    {
      command: "kubernetes/get-pods",
      summary: "List pods in the ingestion namespace",
      params: {
        "pod": "ingester-p4kwm",
      },
    },
  ],
  operation: "kubernetes/restart-pod",
  operationPattern: "kubernetes/*",
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                | Example                                                                                    |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `deploymentId`                                                                             | *string*                                                                                   | :heavy_check_mark:                                                                         | N/A                                                                                        |                                                                                            |
| `remediationPlanId`                                                                        | *string*                                                                                   | :heavy_minus_sign:                                                                         | Set for a plan-backed (ai-agent) request. Omit for a plan-less (CLI) request.              |                                                                                            |
| `title`                                                                                    | *string*                                                                                   | :heavy_minus_sign:                                                                         | Required for a plan-backed request; defaults to the operation/pattern for a plan-less one. |                                                                                            |
| `reason`                                                                                   | *string*                                                                                   | :heavy_minus_sign:                                                                         | N/A                                                                                        |                                                                                            |
| `commands`                                                                                 | [models.CreateAccessRequestCommand](../models/createaccessrequestcommand.md)[]             | :heavy_minus_sign:                                                                         | Plan-backed only: the exact commands the investigation is requesting.                      |                                                                                            |
| `operation`                                                                                | *string*                                                                                   | :heavy_minus_sign:                                                                         | Plan-less, exact request: a single `plugin/operation`.                                     | kubernetes/restart-pod                                                                     |
| `operationPattern`                                                                         | *string*                                                                                   | :heavy_minus_sign:                                                                         | Plan-less, wildcard request: `plugin/*`. Requires `maxRisk`.                               | kubernetes/*                                                                               |
| `params`                                                                                   | *any*                                                                                      | :heavy_minus_sign:                                                                         | Params for an exact `operation` request.                                                   |                                                                                            |
| `maxRisk`                                                                                  | [models.CreateAccessRequestMaxRisk](../models/createaccessrequestmaxrisk.md)               | :heavy_minus_sign:                                                                         | Required with `operationPattern`: the highest risk tier the wildcard grant may cover.      |                                                                                            |