# ServiceAccountSubject

Authenticated service account subject with scoped permissions (workspace, project, or deployment level)

## Example Usage

```typescript
import { ServiceAccountSubject } from "@alienplatform/platform-api/models";

let value: ServiceAccountSubject = {
  kind: "serviceAccount",
  id: "<id>",
  workspaceId: "<id>",
  scope: {
    type: "deployment",
    deploymentId: "<id>",
    projectId: "<id>",
  },
  role: "workspace.member",
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  | Example                                                                      |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `kind`                                                                       | *"serviceAccount"*                                                           | :heavy_check_mark:                                                           | Subject type identifier                                                      |                                                                              |
| `id`                                                                         | *string*                                                                     | :heavy_check_mark:                                                           | Unique service account identifier (API key ID)                               |                                                                              |
| `workspaceId`                                                                | *string*                                                                     | :heavy_check_mark:                                                           | ID of the workspace the service account belongs to                           |                                                                              |
| `scope`                                                                      | *models.SubjectScope*                                                        | :heavy_check_mark:                                                           | Authorization scope defining what resources this service account can access  |                                                                              |
| `role`                                                                       | *models.Role*                                                                | :heavy_check_mark:                                                           | Role defining what actions this service account can perform within its scope | workspace.member                                                             |