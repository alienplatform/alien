# TargetDeploymentManagementConfigUnion


## Supported Types

### `models.TargetDeploymentManagementConfigAws`

```typescript
const value: models.TargetDeploymentManagementConfigAws = {
  managingRoleArn: "<value>",
  platform: "aws",
};
```

### `models.TargetDeploymentManagementConfigGcp`

```typescript
const value: models.TargetDeploymentManagementConfigGcp = {
  serviceAccountEmail: "<value>",
  platform: "gcp",
};
```

### `models.TargetDeploymentManagementConfigAzure`

```typescript
const value: models.TargetDeploymentManagementConfigAzure = {
  managingTenantId: "<id>",
  oidcIssuer: "<value>",
  oidcSubject: "<value>",
  platform: "azure",
};
```

### `models.TargetDeploymentManagementConfigKubernetes`

```typescript
const value: models.TargetDeploymentManagementConfigKubernetes = {
  platform: "kubernetes",
};
```

### `any`

```typescript
const value: any = "<value>";
```

