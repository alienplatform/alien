# Scope

Scope and role configuration for service accounts


## Supported Types

### `models.WorkspaceScope`

```typescript
const value: models.WorkspaceScope = {
  type: "workspace",
  role: "workspace.member",
};
```

### `models.ProjectScope`

```typescript
const value: models.ProjectScope = {
  type: "project",
  projectId: "<id>",
  role: "project.viewer",
};
```

### `models.DeploymentScope`

```typescript
const value: models.DeploymentScope = {
  type: "deployment",
  deploymentId: "<id>",
  projectId: "<id>",
  role: "deployment.viewer",
};
```

### `models.DeploymentGroupScope`

```typescript
const value: models.DeploymentGroupScope = {
  type: "deployment-group",
  deploymentGroupId: "<id>",
  projectId: "<id>",
  role: "deployment-group.deployer",
};
```

### `models.ManagerScope`

```typescript
const value: models.ManagerScope = {
  type: "manager",
  managerId: "<id>",
  role: "manager.runtime",
};
```

