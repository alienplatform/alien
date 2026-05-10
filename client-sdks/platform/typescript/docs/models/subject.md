# Subject

Authenticated principal that can be either a user (with workspace-scoped permissions) or a service account (with configurable scope and role)


## Supported Types

### `models.UserSubject`

```typescript
const value: models.UserSubject = {
  kind: "user",
  id: "<id>",
  email: "King27@yahoo.com",
  workspaceId: "<id>",
  role: "workspace.member",
};
```

### `models.ServiceAccountSubject`

```typescript
const value: models.ServiceAccountSubject = {
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

