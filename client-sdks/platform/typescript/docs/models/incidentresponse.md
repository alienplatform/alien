# IncidentResponse

## Example Usage

```typescript
import { IncidentResponse } from "@alienplatform/platform-api/models";

let value: IncidentResponse = {
  incident: {
    id: "<id>",
    number: 72445,
    reference: "<value>",
    title: "<value>",
    scopeLabel: "<value>",
    agentSessionId: "<id>",
    sessionStatus: "halted",
    projectId: "<id>",
    deploymentId: "<id>",
    releaseId: null,
    createdAt: new Date("2026-12-04T06:44:39.943Z"),
    updatedAt: new Date("2025-11-29T08:01:37.405Z"),
  },
};
```

## Fields

| Field                                    | Type                                     | Required                                 | Description                              |
| ---------------------------------------- | ---------------------------------------- | ---------------------------------------- | ---------------------------------------- |
| `incident`                               | [models.Incident](../models/incident.md) | :heavy_check_mark:                       | N/A                                      |