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
  expiresAt: new Date("2024-04-19T23:44:15.915Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `token`                                                                                       | *string*                                                                                      | :heavy_check_mark:                                                                            | The API key token                                                                             |
| `deploymentLink`                                                                              | *string*                                                                                      | :heavy_check_mark:                                                                            | Formatted deployment link                                                                     |
| `deploymentGroup`                                                                             | [models.DeploymentGroup](../models/deploymentgroup.md)                                        | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `expiresAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | The persisted expiration date for the setup link                                              |