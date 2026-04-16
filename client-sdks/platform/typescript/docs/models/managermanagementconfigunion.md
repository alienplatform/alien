# ManagerManagementConfigUnion

Management configuration for cross-account access (self-reported via heartbeat)


## Supported Types

### `models.ManagerManagementConfigAws`

```typescript
const value: models.ManagerManagementConfigAws = {
  managingRoleArn: "<value>",
  platform: "aws",
};
```

### `models.ManagerManagementConfigGcp`

```typescript
const value: models.ManagerManagementConfigGcp = {
  serviceAccountEmail: "<value>",
  platform: "gcp",
};
```

### `models.ManagerManagementConfigAzure`

```typescript
const value: models.ManagerManagementConfigAzure = {
  managementPrincipalId: "<id>",
  managingTenantId: "<id>",
  platform: "azure",
};
```

### `models.ManagerManagementConfigKubernetes`

```typescript
const value: models.ManagerManagementConfigKubernetes = {
  platform: "kubernetes",
};
```

### `any`

```typescript
const value: any = "<value>";
```

