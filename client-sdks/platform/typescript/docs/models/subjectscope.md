# SubjectScope

Authorization scope defining what resources this service account can access


## Supported Types

### `models.SubjectScopeWorkspace`

```typescript
const value: models.SubjectScopeWorkspace = {
  type: "workspace",
};
```

### `models.SubjectScopeProject`

```typescript
const value: models.SubjectScopeProject = {
  type: "project",
  projectId: "<id>",
};
```

### `models.SubjectScopeDeployment`

```typescript
const value: models.SubjectScopeDeployment = {
  type: "deployment",
  deploymentId: "<id>",
  projectId: "<id>",
};
```

### `models.SubjectScopeDeploymentGroup`

```typescript
const value: models.SubjectScopeDeploymentGroup = {
  type: "deployment-group",
  deploymentGroupId: "<id>",
  projectId: "<id>",
};
```

### `models.SubjectScopeManager`

```typescript
const value: models.SubjectScopeManager = {
  type: "manager",
  managerId: "<id>",
};
```

