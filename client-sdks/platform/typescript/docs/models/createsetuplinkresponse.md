# CreateSetupLinkResponse

## Example Usage

```typescript
import { CreateSetupLinkResponse } from "@alienplatform/platform-api/models";

let value: CreateSetupLinkResponse = {
  token: "<value>",
  deploymentLink: "<value>",
  deploymentGroup: {
    id: "dg_r27ict8c7vcgsumpj90ackf7b",
    name: "prod-us-east-1",
    externalId: "ext_example_01",
    projectId: "prj_mcytp6z3j91f7tn5ryqsfwtr",
    workspaceId: "ws_It13CUaGEhLLAB87simX0",
    createdAt: new Date("2024-09-30T20:33:41.410Z"),
  },
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `token`                                                | *string*                                               | :heavy_check_mark:                                     | The API key token                                      |
| `deploymentLink`                                       | *string*                                               | :heavy_check_mark:                                     | Formatted deployment link                              |
| `deploymentGroup`                                      | [models.DeploymentGroup](../models/deploymentgroup.md) | :heavy_check_mark:                                     | N/A                                                    |