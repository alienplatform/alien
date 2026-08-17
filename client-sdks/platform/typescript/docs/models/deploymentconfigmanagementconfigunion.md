# DeploymentConfigManagementConfigUnion


## Supported Types

### `models.DeploymentConfigManagementConfigAws`

```typescript
const value: models.DeploymentConfigManagementConfigAws = {
  managingRoleArn: "<value>",
  platform: "aws",
};
```

### `models.DeploymentConfigManagementConfigGcp`

```typescript
const value: models.DeploymentConfigManagementConfigGcp = {
  serviceAccountEmail: "<value>",
  platform: "gcp",
};
```

### `models.DeploymentConfigManagementConfigAzure`

```typescript
const value: models.DeploymentConfigManagementConfigAzure = {
  managingTenantId: "<id>",
  oidcIssuer: "<value>",
  oidcSubject: "<value>",
  platform: "azure",
};
```

### `models.DeploymentConfigManagementConfigKubernetes`

```typescript
const value: models.DeploymentConfigManagementConfigKubernetes = {
  platform: "kubernetes",
};
```

### `any`

```typescript
const value: any = "<value>";
```

