# EnsureDeploymentGroupByExternalIdRequest

## Example Usage

```typescript
import { EnsureDeploymentGroupByExternalIdRequest } from "@alienplatform/platform-api/models/operations";

let value: EnsureDeploymentGroupByExternalIdRequest = {
  workspace: "my-workspace",
  ensureDeploymentGroupByExternalIdRequest: {
    externalId: "ext_example_01",
    name: "prod-us-east-1",
    project: "<value>",
  },
};
```

## Fields

| Field                                                                                                                                                                                  | Type                                                                                                                                                                                   | Required                                                                                                                                                                               | Description                                                                                                                                                                            | Example                                                                                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `workspace`                                                                                                                                                                            | *string*                                                                                                                                                                               | :heavy_minus_sign:                                                                                                                                                                     | Workspace name. Required for user/session/OAuth requests. Optional for API keys because API keys are workspace-scoped; if provided with an API key, it must match the key's workspace. | my-workspace                                                                                                                                                                           |
| `ensureDeploymentGroupByExternalIdRequest`                                                                                                                                             | [models.EnsureDeploymentGroupByExternalIdRequest](../../models/ensuredeploymentgroupbyexternalidrequest.md)                                                                            | :heavy_check_mark:                                                                                                                                                                     | N/A                                                                                                                                                                                    |                                                                                                                                                                                        |