# WorkspaceGatewayOverview

## Example Usage

```typescript
import { WorkspaceGatewayOverview } from "@alienplatform/platform-api/models";

let value: WorkspaceGatewayOverview = {
  generatedAt: new Date("2024-01-08T12:06:04.921Z"),
  summary: {
    totalProjects: 269470,
    gatewayProjects: 760669,
    activeProjects: 65837,
    setupInProgressProjects: 360348,
    needsAttentionProjects: 524327,
    connectedCustomers: 97991,
  },
  projects: [],
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `generatedAt`                                                                                 | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `summary`                                                                                     | [models.WorkspaceGatewayOverviewSummary](../models/workspacegatewayoverviewsummary.md)        | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `projects`                                                                                    | [models.GatewayProjectOverview](../models/gatewayprojectoverview.md)[]                        | :heavy_check_mark:                                                                            | N/A                                                                                           |